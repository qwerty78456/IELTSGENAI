//! Google Gemini client on the Interactions API, for text, JSON and speech.
//!
//! Every request is `POST /v1beta/interactions` with `"store": false`: the app
//! keeps no conversation on Google's side, and Google would otherwise store
//! each interaction (55 days on the paid tier). The API key travels in the
//! `x-goog-api-key` header, never in the URL, so it cannot leak through logs
//! or proxies.
//!
//! Every billed response is added to the client's usage meter before it is
//! parsed, so a reply that is cut off or malformed is still counted.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::domain::Usage;

use super::super::audio::{Pcm16, SAMPLE_RATE};
use super::super::config::config;
use super::super::jobs::now_secs;
use super::pricing::{cost_micro_usd, rates_for};

const API_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta";
const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const TEXT_TIMEOUT: Duration = Duration::from_secs(90);
const TTS_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RETRIES: u32 = 3;
const FIRST_BACKOFF_MS: u64 = 1_000;
/// Cap on one text answer, thinking included. Every real answer fits in a
/// fraction of it; it only stops a runaway reply from running up the bill.
const MAX_OUTPUT_TOKENS: u32 = 8_192;
/// Audio tokens billed per second of speech: 32, measured with
/// gemini-3.8-flash-tts on 2026-09-28 (the pricing page says 25). Used only
/// when a speech response reports no output tokens.
pub const AUDIO_TOKENS_PER_SECOND: u64 = 32;
/// Error codes with which Gemini refuses content rather than failing.
const BLOCK_CODES: [&str; 6] = [
    "safety",
    "recitation",
    "prohibited_content",
    "spii",
    "blocklist",
    "language",
];

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error(
        "The AI service is not configured: set GEMINI_API_KEY in the environment or .env file, or enter the key at the top of the page."
    )]
    NoApiKey,
    #[error("Google rejected this API key. Check it in Google AI Studio and try again.")]
    KeyRejected,
    #[error("The AI service is busy right now. Please try again in a minute.")]
    Busy,
    #[error("The AI service did not answer in time. Please try again.")]
    Timeout,
    #[error("The AI service rejected the request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("Gemini refused to write this ({0}). Try another topic or wording.")]
    Blocked(String),
    #[error("Network error while contacting the AI service: {0}")]
    Network(String),
    #[error("The AI service returned something unexpected: {0}")]
    Malformed(String),
}

/// A passage label bound to a prebuilt Gemini voice name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceAssignment {
    pub label: String,
    pub voice: String,
}

/// One stretch of speech, read word for word. `speaker` is a passage label
/// ("Speaker A") and is required when the request has two voices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechTurn {
    pub speaker: Option<String>,
    pub text: String,
}

/// One speech request: its turns, one or two voices, and how to deliver them.
/// Gemini TTS reads text verbatim, so directions go in `style`, never in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechRequest {
    pub turns: Vec<SpeechTurn>,
    pub voices: Vec<VoiceAssignment>,
    pub style: String,
}

#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
    text_model: String,
    tts_model: String,
    thinking_level: String,
    /// Shared by clones, so the parts of one exam job add to one total.
    meter: Arc<Mutex<Usage>>,
}

impl GeminiClient {
    /// Checks a key before it is kept: one free model-list request, no tokens billed.
    pub async fn check_key(api_key: &str) -> Result<(), LlmError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;
        let sent = http
            .get(format!("{API_ROOT}/models?pageSize=1"))
            .header("x-goog-api-key", api_key)
            .timeout(KEY_CHECK_TIMEOUT)
            .send()
            .await;
        match sent {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => match response.status().as_u16() {
                400 | 401 | 403 => Err(LlmError::KeyRejected),
                429 | 503 => Err(LlmError::Busy),
                status => Err(LlmError::Rejected {
                    status,
                    body: "the key could not be checked".into(),
                }),
            },
            Err(e) if e.is_timeout() => Err(LlmError::Timeout),
            Err(e) => Err(LlmError::Network(e.to_string())),
        }
    }

    pub fn from_config() -> Result<Self, LlmError> {
        let cfg = config();
        let api_key = super::super::secrets::api_key().ok_or(LlmError::NoApiKey)?;
        Self::new(
            api_key,
            cfg.text_model.clone(),
            cfg.tts_model.clone(),
            cfg.thinking_level.clone(),
        )
    }

    pub(crate) fn new(
        api_key: String,
        text_model: String,
        tts_model: String,
        thinking_level: String,
    ) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;
        Ok(Self {
            http,
            api_key,
            text_model,
            tts_model,
            thinking_level,
            meter: Arc::new(Mutex::new(Usage::default())),
        })
    }

    pub fn text_model(&self) -> &str {
        &self.text_model
    }

    pub fn tts_model(&self) -> &str {
        &self.tts_model
    }

    /// Everything this client (and its clones) has been billed for so far.
    pub fn usage(&self) -> Usage {
        self.meter.lock().map(|usage| *usage).unwrap_or_default()
    }

    /// Adds usage that did not come from a response, such as a speech request
    /// answered from the local cache.
    pub fn add_usage(&self, usage: &Usage) {
        if let Ok(mut meter) = self.meter.lock() {
            meter.add(usage);
        }
    }

    /// Plain text completion.
    pub async fn generate_text(&self, prompt: &str) -> Result<String, LlmError> {
        let body = text_body(&self.text_model, prompt, &self.thinking_level, false);
        let response = self.billed(&self.text_model, body, TEXT_TIMEOUT).await?;
        output_text(&response).ok_or_else(|| LlmError::Malformed("no text in the response".into()))
    }

    /// JSON completion parsed into `T`. Markdown fences around the payload are tolerated.
    pub async fn generate_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T, LlmError> {
        let json_prompt =
            format!("{prompt}\n\nRespond with a single JSON object and nothing else.");
        let body = text_body(&self.text_model, &json_prompt, &self.thinking_level, true);
        let response = self.billed(&self.text_model, body, TEXT_TIMEOUT).await?;
        let text = output_text(&response)
            .ok_or_else(|| LlmError::Malformed("no text in the response".into()))?;
        let payload = strip_fences(&text);
        serde_json::from_str(payload).map_err(|e| {
            LlmError::Malformed(format!(
                "{e}; payload starts with: {}",
                payload.chars().take(200).collect::<String>()
            ))
        })
    }

    /// Speech synthesis: 24 kHz mono PCM. At most two voices per request.
    pub async fn synthesize(&self, request: &SpeechRequest) -> Result<Pcm16, LlmError> {
        let body = speech_body(&self.tts_model, request)?;
        let response = self.billed(&self.tts_model, body, TTS_TIMEOUT).await?;
        let pcm = output_audio(&response)?;
        if tokens_of(&response).output == 0 {
            // No count reported: bill what the audio length implies.
            let output_tokens = u64::from(pcm.duration_ms()) * AUDIO_TOKENS_PER_SECOND / 1_000;
            let micro_usd = rates_for(&self.tts_model, now_secs())
                .map(|rates| cost_micro_usd(rates, 0, 0, output_tokens, 0))
                .unwrap_or(0);
            self.add_usage(&Usage {
                output_tokens,
                micro_usd,
                ..Usage::default()
            });
        }
        Ok(pcm)
    }

    /// Sends a request, meters the response, then checks that it finished.
    async fn billed(&self, model: &str, body: Value, timeout: Duration) -> Result<Value, LlmError> {
        let started = Instant::now();
        let response = self.call(model, body, timeout).await?;
        let usage = priced_usage(model, &response, now_secs());
        let served_by = response.get("model").and_then(Value::as_str).unwrap_or("");
        tracing::info!(
            model,
            served_by,
            input = usage.input_tokens,
            cached = usage.cached_tokens,
            output = usage.output_tokens,
            thinking = usage.thinking_tokens,
            micro_usd = usage.micro_usd,
            latency_ms = started.elapsed().as_millis() as u64,
            "Gemini request"
        );
        self.add_usage(&usage);
        check_status(&response)?;
        Ok(response)
    }

    async fn call(&self, model: &str, body: Value, timeout: Duration) -> Result<Value, LlmError> {
        let url = format!("{API_ROOT}/interactions");
        let mut backoff_ms = FIRST_BACKOFF_MS;
        for attempt in 0..=MAX_RETRIES {
            let sent = self
                .http
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .timeout(timeout)
                .json(&body)
                .send()
                .await;
            let retry_reason = match sent {
                Ok(response) if response.status().is_success() => {
                    return response
                        .json::<Value>()
                        .await
                        .map_err(|e| LlmError::Malformed(e.to_string()));
                }
                Ok(response) if matches!(response.status().as_u16(), 429 | 503 | 504) => {
                    LlmError::Busy
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response.text().await.unwrap_or_default();
                    return Err(error_of(status, &body));
                }
                Err(e) if e.is_timeout() => LlmError::Timeout,
                Err(e) => return Err(LlmError::Network(e.to_string())),
            };
            if attempt == MAX_RETRIES {
                return Err(retry_reason);
            }
            tracing::warn!(
                model,
                attempt,
                backoff_ms,
                "Gemini call will be retried: {retry_reason}"
            );
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms *= 2;
        }
        Err(LlmError::Busy)
    }
}

/// A text request. `thinking_level` is low, medium or high (3.8 Flash
/// rejects `minimal` and cannot turn thinking off).
fn text_body(model: &str, prompt: &str, thinking_level: &str, json_mode: bool) -> Value {
    let mut body = json!({
        "model": model,
        "input": prompt,
        "generation_config": {
            "thinking_level": thinking_level,
            "max_output_tokens": MAX_OUTPUT_TOKENS,
        },
        "store": false,
    });
    if json_mode {
        body["response_format"] = json!({ "type": "text", "mime_type": "application/json" });
    }
    body
}

/// Speaker names on the wire: the label without spaces ("Speaker A" →
/// "SpeakerA"). They are never spoken; the domain keeps its labels.
fn wire_speaker(label: &str) -> String {
    label.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A speech request: one `text` item per turn, each carrying the delivery
/// style (and, with two voices, its speaker) as `speech_metadata`, and raw
/// 24 kHz PCM requested instead of the default WAV.
fn speech_body(model: &str, request: &SpeechRequest) -> Result<Value, LlmError> {
    if request.turns.is_empty() {
        return Err(LlmError::Malformed("nothing to read aloud".into()));
    }
    let speech_config = match request.voices.as_slice() {
        [] => return Err(LlmError::Malformed("no voice assigned".into())),
        [single] => json!([{ "voice": single.voice }]),
        [first, second] => {
            let speakers: Vec<Value> = [first, second]
                .iter()
                .map(|v| json!({ "speaker": wire_speaker(&v.label), "voice": v.voice }))
                .collect();
            json!({ "mode": "conversational", "speakers": speakers })
        }
        _ => {
            return Err(LlmError::Malformed(
                "at most two voices per speech request".into(),
            ));
        }
    };
    let two_voices = request.voices.len() == 2;
    let mut content = Vec::with_capacity(request.turns.len());
    for turn in &request.turns {
        let mut metadata = json!({ "type": "speech_metadata", "style": request.style });
        if two_voices {
            let speaker = turn
                .speaker
                .as_deref()
                .filter(|label| request.voices.iter().any(|v| v.label == *label))
                .ok_or_else(|| {
                    LlmError::Malformed(format!(
                        "turn by {:?} has no voice in this request",
                        turn.speaker
                    ))
                })?;
            metadata["speaker"] = json!(wire_speaker(speaker));
        }
        content.push(json!({ "type": "text", "text": turn.text, "annotations": [metadata] }));
    }
    Ok(json!({
        "model": model,
        "input": [{ "type": "user_input", "content": content }],
        "response_format": { "type": "audio", "mime_type": "audio/l16", "sample_rate": SAMPLE_RATE },
        "generation_config": { "speech_config": speech_config },
        "store": false,
    }))
}

/// Output steps of an interaction; `thought` steps are skipped.
fn model_outputs(response: &Value) -> impl Iterator<Item = &Value> {
    response
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|step| step.get("type").and_then(Value::as_str) == Some("model_output"))
}

fn content_of<'a>(step: &'a Value, kind: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    step.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(move |item| item.get("type").and_then(Value::as_str) == Some(kind))
}

/// The text of the last output step, trimmed; `None` when there is none.
fn output_text(response: &Value) -> Option<String> {
    let step = model_outputs(response).last()?;
    let text: String = content_of(step, "text")
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Every audio item of the output, decoded and joined. Raw L16 is what the
/// request asks for; a RIFF/WAVE payload (the API's default) is parsed too.
fn output_audio(response: &Value) -> Result<Pcm16, LlmError> {
    let mut out = Pcm16::silence(0, SAMPLE_RATE);
    let mut found = false;
    for step in model_outputs(response) {
        for item in content_of(step, "audio") {
            let data = item
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| LlmError::Malformed("audio without inline data".into()))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| LlmError::Malformed(format!("audio is not valid base64: {e}")))?;
            let pcm = if bytes.starts_with(b"RIFF") {
                Pcm16::from_wav(&bytes).map_err(LlmError::Malformed)?
            } else {
                let rate = item
                    .get("sample_rate")
                    .and_then(Value::as_u64)
                    .map_or(SAMPLE_RATE, |rate| rate as u32);
                Pcm16::from_le_bytes(&bytes, rate)
            };
            if pcm.sample_rate != SAMPLE_RATE {
                return Err(LlmError::Malformed(format!(
                    "audio came back at {} Hz; {SAMPLE_RATE} Hz was requested",
                    pcm.sample_rate
                )));
            }
            out.append(&pcm);
            found = true;
        }
    }
    if found {
        Ok(out)
    } else {
        Err(LlmError::Malformed("no audio in the response".into()))
    }
}

/// Token counts of one response. Missing fields count as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TokenCount {
    input: u64,
    cached: u64,
    output: u64,
    thinking: u64,
}

fn tokens_of(response: &Value) -> TokenCount {
    let usage = response.get("usage");
    let count = |field: &str| {
        usage
            .and_then(|u| u.get(field))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    TokenCount {
        input: count("total_input_tokens"),
        cached: count("total_cached_tokens"),
        output: count("total_output_tokens"),
        thinking: count("total_thought_tokens"),
    }
}

/// One billed response as `Usage`, priced for the requested model or, failing
/// that, the model that served it.
fn priced_usage(model: &str, response: &Value, at_secs: i64) -> Usage {
    let tokens = tokens_of(response);
    let served = response.get("model").and_then(Value::as_str);
    let rates = rates_for(model, at_secs).or_else(|| served.and_then(|m| rates_for(m, at_secs)));
    Usage {
        requests: 1,
        input_tokens: tokens.input,
        cached_tokens: tokens.cached,
        output_tokens: tokens.output,
        thinking_tokens: tokens.thinking,
        micro_usd: rates
            .map(|r| {
                cost_micro_usd(
                    r,
                    tokens.input,
                    tokens.cached,
                    tokens.output,
                    tokens.thinking,
                )
            })
            .unwrap_or(0),
        unpriced: u32::from(rates.is_none()),
        ..Usage::default()
    }
}

/// A completed interaction passes; a cut-off, failed or refused one becomes
/// a readable error.
fn check_status(response: &Value) -> Result<(), LlmError> {
    let status = response.get("status").and_then(Value::as_str);
    match status {
        None | Some("completed") => Ok(()),
        Some("incomplete") => Err(LlmError::Malformed(
            "the answer was cut off before it finished".into(),
        )),
        Some(other) => {
            let error = response.pointer("/errors/0");
            let code = error
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let message = error
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("no details");
            if let Some(block) = blocked(code) {
                return Err(LlmError::Blocked(block));
            }
            Err(LlmError::Malformed(format!(
                "interaction {other}: {message}"
            )))
        }
    }
}

/// The content-block reason in an error code such as `safety` (or a URI ending in it).
fn blocked(code: &str) -> Option<String> {
    let last = code.rsplit(['/', '#', ':']).next().unwrap_or(code);
    BLOCK_CODES
        .iter()
        .find(|block| last.eq_ignore_ascii_case(block))
        .map(|block| block.to_string())
}

/// A non-2xx response: `{"error": {"code": "...", "message": "..."}}`.
fn error_of(status: u16, body: &str) -> LlmError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let error = parsed.as_ref().and_then(|v| v.get("error"));
    let code = error
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(block) = blocked(code) {
        return LlmError::Blocked(block);
    }
    let message = error
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or(body);
    LlmError::Rejected {
        status,
        body: message.chars().take(500).collect(),
    }
}

fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(without_open) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let body = without_open
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    body.trim_end().strip_suffix("```").unwrap_or(body).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(label: &str, voice: &str) -> VoiceAssignment {
        VoiceAssignment {
            label: label.into(),
            voice: voice.into(),
        }
    }

    fn turn(speaker: Option<&str>, text: &str) -> SpeechTurn {
        SpeechTurn {
            speaker: speaker.map(Into::into),
            text: text.into(),
        }
    }

    #[test]
    fn fences_are_removed() {
        assert_eq!(strip_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fences("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn text_requests_are_not_stored_and_think_little() {
        let body = text_body("gemini-3.8-flash", "Write.", "low", true);
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["input"], json!("Write."));
        assert_eq!(body["generation_config"]["thinking_level"], json!("low"));
        assert_eq!(
            body["generation_config"]["max_output_tokens"],
            json!(MAX_OUTPUT_TOKENS)
        );
        assert_eq!(
            body["response_format"]["mime_type"],
            json!("application/json")
        );
        assert!(
            text_body("m", "p", "low", false)
                .get("response_format")
                .is_none()
        );
    }

    #[test]
    fn two_voice_speech_sends_one_annotated_item_per_turn() {
        let request = SpeechRequest {
            turns: vec![
                turn(Some("Speaker A"), "Good morning."),
                turn(Some("Speaker B"), "Hello."),
            ],
            voices: vec![voice("Speaker A", "Kore"), voice("Speaker B", "Puck")],
            style: "calm".into(),
        };
        let body = speech_body("gemini-3.8-flash-tts", &request).unwrap();
        assert_eq!(body["store"], json!(false));
        assert_eq!(
            body["generation_config"]["speech_config"],
            json!({
                "mode": "conversational",
                "speakers": [
                    { "speaker": "SpeakerA", "voice": "Kore" },
                    { "speaker": "SpeakerB", "voice": "Puck" },
                ],
            })
        );
        let content = &body["input"][0]["content"];
        assert_eq!(body["input"][0]["type"], json!("user_input"));
        assert_eq!(content[0]["text"], json!("Good morning."));
        assert_eq!(
            content[1]["annotations"][0],
            json!({ "type": "speech_metadata", "style": "calm", "speaker": "SpeakerB" })
        );
        assert_eq!(body["response_format"]["mime_type"], json!("audio/l16"));
        assert_eq!(body["response_format"]["sample_rate"], json!(24_000));
    }

    #[test]
    fn single_voice_speech_names_no_speaker() {
        let request = SpeechRequest {
            turns: vec![turn(None, "Part one.")],
            voices: vec![voice("Announcer", "Charon")],
            style: "slow".into(),
        };
        let body = speech_body("m", &request).unwrap();
        assert_eq!(
            body["generation_config"]["speech_config"],
            json!([{ "voice": "Charon" }])
        );
        assert_eq!(
            body["input"][0]["content"][0]["annotations"][0],
            json!({ "type": "speech_metadata", "style": "slow" })
        );
    }

    #[test]
    fn speech_requests_are_checked_before_sending() {
        let mut request = SpeechRequest {
            turns: vec![turn(Some("Speaker C"), "Who am I?")],
            voices: vec![voice("Speaker A", "Kore"), voice("Speaker B", "Puck")],
            style: "calm".into(),
        };
        assert!(speech_body("m", &request).is_err(), "unknown speaker");
        request.voices.push(voice("Speaker C", "Fenrir"));
        assert!(speech_body("m", &request).is_err(), "three voices");
        request.voices.clear();
        assert!(speech_body("m", &request).is_err(), "no voice");
    }

    #[test]
    fn text_comes_from_the_last_output_step_without_thoughts() {
        let response = json!({
            "status": "completed",
            "steps": [
                { "type": "thought", "summary": "thinking..." },
                { "type": "model_output", "content": [
                    { "type": "text", "text": "  Hello " },
                    { "type": "text", "text": "world.  " },
                ]},
            ],
        });
        assert_eq!(output_text(&response).as_deref(), Some("Hello world."));
        assert_eq!(output_text(&json!({ "steps": [] })), None);
    }

    #[test]
    fn audio_is_decoded_from_l16_or_wav() {
        let pcm = Pcm16::silence(100, SAMPLE_RATE);
        let raw: Vec<u8> = pcm.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let response = json!({ "steps": [{ "type": "model_output", "content": [
            { "type": "audio", "data": b64(&raw), "mime_type": "audio/l16", "sample_rate": 24_000 },
            { "type": "audio", "data": b64(&pcm.to_wav()), "mime_type": "audio/wav" },
        ]}]});
        assert_eq!(output_audio(&response).unwrap().duration_ms(), 200);
        let wrong_rate = json!({ "steps": [{ "type": "model_output", "content": [
            { "type": "audio", "data": b64(&Pcm16::silence(10, 16_000).to_wav()) },
        ]}]});
        assert!(output_audio(&wrong_rate).is_err());
        assert!(output_audio(&json!({ "steps": [] })).is_err());
    }

    #[test]
    fn usage_is_priced_with_thinking_and_cache() {
        let response = json!({
            "model": "gemini-3.8-flash",
            "usage": {
                "total_input_tokens": 2_000,
                "total_cached_tokens": 1_000,
                "total_output_tokens": 400,
                "total_thought_tokens": 600,
                "total_tokens": 3_000,
            },
        });
        let usage = priced_usage("gemini-3.8-flash", &response, 0);
        assert_eq!(usage.requests, 1);
        assert_eq!(usage.thinking_tokens, 600);
        assert_eq!(usage.micro_usd, 750 + 75 + 3_750);
        // An alias is priced through the model that served it.
        assert_eq!(
            priced_usage("gemini-flash-latest", &response, 0).micro_usd,
            4_575
        );
        let unknown = priced_usage("some-model", &json!({ "usage": {} }), 0);
        assert_eq!((unknown.unpriced, unknown.micro_usd), (1, 0));
    }

    #[test]
    fn unfinished_and_refused_interactions_are_errors() {
        assert!(check_status(&json!({ "status": "completed" })).is_ok());
        assert!(matches!(
            check_status(&json!({ "status": "incomplete" })),
            Err(LlmError::Malformed(message)) if message.contains("cut off")
        ));
        assert!(matches!(
            check_status(&json!({ "status": "failed", "errors": [{ "code": "https://x/errors/safety", "message": "no" }] })),
            Err(LlmError::Blocked(code)) if code == "safety"
        ));
        assert!(matches!(
            error_of(
                400,
                r#"{"error":{"code":"prohibited_content","message":"x"}}"#
            ),
            LlmError::Blocked(_)
        ));
        assert!(matches!(
            error_of(400, r#"{"error":{"code":"invalid_request","message":"bad field"}}"#),
            LlmError::Rejected { status: 400, body } if body == "bad field"
        ));
        assert!(matches!(
            error_of(500, "not json"),
            LlmError::Rejected { .. }
        ));
    }

    /// Calls the real API with the key from the environment (process, then
    /// Windows). Costs about $0.03. Run on demand:
    /// `cargo test --features server --no-default-features live_probe -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_probe() {
        let key = std::env::var("GEMINI_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| super::super::super::config::windows_environment().remove("GEMINI_API_KEY"))
            .expect("GEMINI_API_KEY in the environment");
        let text_model = std::env::var("GEMINI_TEXT_MODEL").unwrap_or("gemini-3.8-flash".into());
        let tts_model = std::env::var("GEMINI_TTS_MODEL").unwrap_or("gemini-3.8-flash-tts".into());
        let level = std::env::var("GEMINI_THINKING_LEVEL").unwrap_or("low".into());
        let client = GeminiClient::new(key, text_model.clone(), tts_model.clone(), level).unwrap();
        let report = |label: &str, before: Usage, started: Instant, audio_ms: Option<u32>| {
            let after = client.usage();
            let spent = Usage {
                requests: after.requests - before.requests,
                input_tokens: after.input_tokens - before.input_tokens,
                cached_tokens: after.cached_tokens - before.cached_tokens,
                output_tokens: after.output_tokens - before.output_tokens,
                thinking_tokens: after.thinking_tokens - before.thinking_tokens,
                micro_usd: after.micro_usd - before.micro_usd,
                unpriced: after.unpriced - before.unpriced,
                ..Usage::default()
            };
            let per_second = audio_ms
                .filter(|ms| *ms > 0)
                .map(|ms| {
                    format!(
                        ", {:.1} tokens/s of audio over {:.1} s",
                        spent.output_tokens as f64 * 1000.0 / f64::from(ms),
                        f64::from(ms) / 1000.0
                    )
                })
                .unwrap_or_default();
            println!(
                "{label}: {:.1} s, in {} (cached {}), out {}, thinking {}, {}{per_second}",
                started.elapsed().as_secs_f32(),
                spent.input_tokens,
                spent.cached_tokens,
                spent.output_tokens,
                spent.thinking_tokens,
                spent.cost_text()
            );
        };

        let (before, started) = (client.usage(), Instant::now());
        let text = client
            .generate_text("Suggest one everyday topic for an IELTS Listening Part 1 conversation. Answer with the topic only.")
            .await
            .unwrap();
        report(
            &format!("text ({text_model}) -> {text:?}"),
            before,
            started,
            None,
        );

        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct Probe {
            topic: String,
            words: u32,
        }
        let (before, started) = (client.usage(), Instant::now());
        let parsed: Probe = client
            .generate_json(r#"Return {"topic": <a short topic>, "words": <its word count>}."#)
            .await
            .unwrap();
        report(&format!("json -> {parsed:?}"), before, started, None);

        let dir = std::env::temp_dir();
        let (before, started) = (client.usage(), Instant::now());
        let single = client
            .synthesize(&SpeechRequest {
                turns: vec![turn(
                    None,
                    "Part one. You will hear a conversation between a receptionist and a caller.",
                )],
                voices: vec![voice("Announcer", "Charon")],
                style: "slow and clear, like an exam announcer".into(),
            })
            .await
            .unwrap();
        std::fs::write(dir.join("ielts-probe-single.wav"), single.to_wav()).unwrap();
        report(
            &format!("tts one voice ({tts_model})"),
            before,
            started,
            Some(single.duration_ms()),
        );

        let dialogue = [
            (
                "Speaker A",
                "Good morning, Riverside Sports Centre. My name is Sarah. How can I help you today?",
            ),
            (
                "Speaker B",
                "Hi, I'd like to ask about joining the swimming club. I saw a poster in the library last week.",
            ),
            (
                "Speaker A",
                "Of course. Membership is forty-five pounds a month, or four hundred and twenty for the whole year.",
            ),
            (
                "Speaker B",
                "That sounds reasonable. Do I need to bring anything for the first session, like a photo?",
            ),
            (
                "Speaker A",
                "Just some identification and a passport-sized photo. The first session is on Tuesday at seven.",
            ),
        ];
        let (before, started) = (client.usage(), Instant::now());
        let pair = client
            .synthesize(&SpeechRequest {
                turns: dialogue.iter().map(|(s, t)| turn(Some(s), t)).collect(),
                voices: vec![voice("Speaker A", "Kore"), voice("Speaker B", "Puck")],
                style: "natural, clear pronunciation at a steady exam pace".into(),
            })
            .await
            .unwrap();
        std::fs::write(dir.join("ielts-probe-dialogue.wav"), pair.to_wav()).unwrap();
        report("tts two voices", before, started, Some(pair.duration_ms()));

        // The raw usage object of one speech response, audio data left out.
        let body = speech_body(
            &tts_model,
            &SpeechRequest {
                turns: vec![turn(None, "Thank you.")],
                voices: vec![voice("Announcer", "Charon")],
                style: "calm".into(),
            },
        )
        .unwrap();
        let raw = client.call(&tts_model, body, TTS_TIMEOUT).await.unwrap();
        client.add_usage(&priced_usage(&tts_model, &raw, now_secs()));
        println!(
            "raw speech usage: {}",
            raw.get("usage").cloned().unwrap_or_default()
        );
        println!(
            "raw speech status/model: {:?} / {:?}",
            raw.get("status"),
            raw.get("model")
        );
        println!("total: {}", client.usage().cost_text());
        println!("recordings: {}", dir.join("ielts-probe-*.wav").display());
    }
}
