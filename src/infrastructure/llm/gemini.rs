//! Google Gemini client on the Interactions API, for text, JSON and speech,
//! plus the free voice catalogue.
//!
//! Every generation request is `POST /v1beta/interactions` with
//! `"store": false`: the app keeps no conversation on Google's side, and
//! Google would otherwise store each interaction (55 days on the paid tier).
//! The Voices API is `GET /v1beta/voices` (the catalogue and this project's
//! designed voices), `GET` and `DELETE /v1beta/voices/{id}`, and
//! `POST /v1beta/voices` with `"store": true` (Voice Design): the only
//! request that keeps something at Google, the designed voice itself. All of
//! them go through `send`, one retry policy. The API key travels in the
//! `x-goog-api-key` header, never in the URL, so it cannot leak through logs
//! or proxies.
//!
//! Every billed response is added to the client's usage meter before it is
//! parsed, so a reply that is cut off or malformed is still counted.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::domain::{Gender, Usage, Voice};

use super::super::audio::{Pcm16, SAMPLE_RATE};
use super::super::config::config;
use super::super::jobs::now_secs;
use super::pricing::{cost_micro_usd, rates_for};

const API_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta";
const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const TEXT_TIMEOUT: Duration = Duration::from_secs(90);
const TTS_TIMEOUT: Duration = Duration::from_secs(300);
const VOICES_TIMEOUT: Duration = Duration::from_secs(30);
/// Voice Design answered in 21 s on 2026-10-05 (the voice and a 20 s sample).
const DESIGN_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RETRIES: u32 = 3;
/// A 503 answered within this is a refusal before any work; a later one may
/// not be, so a request that creates something is not repeated after it.
const QUICK_REFUSAL: Duration = Duration::from_secs(5);
const FIRST_BACKOFF_MS: u64 = 1_000;
/// Longest wait honoured from a 429's `retryDelay`.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
/// Speech requests in flight at once, for the whole process: the parts of an
/// exam job and the chunks of each part all queue here, so a render never
/// fires more requests than the key's per-minute limit tolerates.
const TTS_PARALLEL_REQUESTS: usize = 3;
static TTS_SLOTS: Semaphore = Semaphore::const_new(TTS_PARALLEL_REQUESTS);
/// Voices per catalogue page (the API's maximum) and a stop for a list that
/// never ends.
const VOICE_PAGE_SIZE: u32 = 1_000;
const MAX_VOICE_PAGES: usize = 20;
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
    /// The key requests use was refused; the text says where that key lives.
    #[error(
        "Google rejected the Gemini API key from {0}. Check it in Google AI Studio; the box at the top of the page says how to replace it."
    )]
    ActiveKeyRejected(&'static str),
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
    /// A voice id Google refused: unknown, retired, or a designed voice of
    /// another Google project.
    #[error(
        "Google could not use the voice \"{0}\": it does not exist, or it was designed with another API key (Google project). Choose another voice."
    )]
    UnknownVoice(String),
    /// A string that cannot be a voice id; the text says what one looks like.
    #[error("{0}")]
    NotAVoiceId(String),
    /// Voice Design refused: the project holds as many designed voices as
    /// Google allows, or too many were made just now.
    #[error(
        "Google would not create another designed voice: the Google project of this API key may already hold 200 designed voices (Google's limit), or too many were created just now. Delete designed voices you no longer use, or try again in a few minutes."
    )]
    VoiceLimit,
    /// Voice Design refused the description or the name.
    #[error("Google did not create this voice: {0}. Change the name or description and try again.")]
    VoiceNotCreated(String),
    /// Voice Design did not answer in time. It is never repeated on its own:
    /// the voice may have been made anyway.
    #[error(
        "Google took too long to create the voice. It may still appear among the designed voices in a minute; look there before creating it again."
    )]
    VoiceDesignTimeout,
}

/// A passage label bound to a Gemini voice id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceAssignment {
    pub label: String,
    pub voice: String,
}

/// One stretch of speech, read word for word. `speaker` is a passage label
/// ("Speaker A") and is required when the request has two voices. Gemini TTS
/// reads text verbatim, so delivery directions go in `style`, never in the
/// text; an empty style sends none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechTurn {
    pub speaker: Option<String>,
    pub text: String,
    pub style: String,
}

/// One speech request: its turns and one or two voices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechRequest {
    pub turns: Vec<SpeechTurn>,
    pub voices: Vec<VoiceAssignment>,
}

/// Filters for `GeminiClient::list_voices`; `None` leaves one out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceQuery {
    /// "en-GB".
    pub language_code: Option<String>,
    /// "male" or "female".
    pub gender: Option<String>,
    /// "prebuilt" (the library) or "prompted" (this project's designed voices).
    pub voice_type: Option<String>,
    /// The catalogue's exact accent, such as "Winchester English".
    pub accent: Option<String>,
    /// Voices per page; the API's maximum when unset.
    pub page_size: Option<u32>,
}

impl VoiceQuery {
    /// The query string of one page.
    fn pairs(&self, page_token: Option<&str>) -> Vec<(&'static str, String)> {
        let mut pairs = vec![(
            "page_size",
            self.page_size.unwrap_or(VOICE_PAGE_SIZE).to_string(),
        )];
        for (name, value) in [
            ("language_code", &self.language_code),
            ("gender", &self.gender),
            ("type", &self.voice_type),
            ("accent", &self.accent),
        ] {
            if let Some(value) = value.as_deref().filter(|v| !v.trim().is_empty()) {
                pairs.push((name, value.to_string()));
            }
        }
        if let Some(token) = page_token {
            pairs.push(("page_token", token.to_string()));
        }
        pairs
    }
}

/// One voice of Google's catalogue, as listed. Fields Google leaves out are
/// empty strings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogVoice {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub gender: String,
    pub accent: String,
    pub language_code: String,
    pub region_code: String,
    pub pitch: String,
    pub persona: String,
    pub context: String,
    /// "prebuilt" or "prompted".
    pub voice_type: String,
}

/// What Voice Design is asked to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceDesign {
    pub display_name: String,
    pub gender: Gender,
    /// "en-GB": the accent's language tag.
    pub language_code: String,
    /// One or two sentences: age, timbre, regional accent, pace.
    pub description: String,
}

/// A voice Voice Design made, with the sample Google returns with it.
#[derive(Debug, Clone, PartialEq)]
pub struct CreatedVoice {
    /// "voice_kwq20yi2gjin".
    pub id: String,
    pub display_name: String,
    /// "female" or "male", as Google reports it.
    pub gender: String,
    pub language_code: String,
    /// About 20 s of the voice; `None` when Google sent none or it did not decode.
    pub sample: Option<Pcm16>,
}

/// Which failures `send` repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// 429, 503, 504 and timeouts.
    Always,
    /// 429, and a 503 answered within `QUICK_REFUSAL`: what Google answers
    /// before doing the work. A request that creates something (a stored
    /// voice) is never repeated after a timeout, a 504 or a slow 503: it may
    /// have been made, and billed, already.
    BeforeWork,
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

    /// Tells the Google projects of two keys apart without keeping the key:
    /// designed voices belong to the project, so what was listed with one
    /// key must not be reused with another.
    pub fn project_tag(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.api_key.hash(&mut hasher);
        hasher.finish()
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

    /// Speech synthesis: 24 kHz mono PCM. At most two voices per request, and
    /// at most `TTS_PARALLEL_REQUESTS` requests in flight in the process.
    pub async fn synthesize(&self, request: &SpeechRequest) -> Result<Pcm16, LlmError> {
        let body = speech_body(&self.tts_model, request)?;
        let _slot = TTS_SLOTS.acquire().await.map_err(|_| LlmError::Busy)?;
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

    /// Google's voice catalogue, every page: free, nothing is billed. With
    /// `voice_type` "prompted", the designed voices of this key's project.
    pub async fn list_voices(&self, query: &VoiceQuery) -> Result<Vec<CatalogVoice>, LlmError> {
        let mut voices = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_VOICE_PAGES {
            let page = self
                .send(
                    Method::GET,
                    "voices",
                    &query.pairs(token.as_deref()),
                    None,
                    VOICES_TIMEOUT,
                    Retry::Always,
                )
                .await?;
            voices.extend(
                page.get("voices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(catalog_voice),
            );
            token = page
                .get("next_page_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
                .map(str::to_string);
            if token.is_none() {
                return Ok(voices);
            }
        }
        Err(LlmError::Malformed("the voice list did not end".into()))
    }

    /// One voice with its sample (designed voices carry one; library voices
    /// are not served here and give `UnknownVoice`). Free.
    pub async fn get_voice(&self, id: &str) -> Result<(CatalogVoice, Option<Pcm16>), LlmError> {
        let path = voice_path(id)?;
        let response = self
            .send(Method::GET, &path, &[], None, VOICES_TIMEOUT, Retry::Always)
            .await
            .map_err(|e| unknown_voice(e, id))?;
        let voice = catalog_voice(&response)
            .ok_or_else(|| LlmError::Malformed("the voice came back without an id".into()))?;
        Ok((voice, sample_of(&response)))
    }

    /// Deletes a designed voice of this key's project. Free.
    pub async fn delete_voice(&self, id: &str) -> Result<(), LlmError> {
        let path = voice_path(id)?;
        self.send(
            Method::DELETE,
            &path,
            &[],
            None,
            VOICES_TIMEOUT,
            Retry::Always,
        )
        .await
        .map_err(|e| unknown_voice(e, id))?;
        Ok(())
    }

    /// Voice Design: makes and stores a voice in this key's Google project
    /// (`"store": true`, the only stored request; up to 200 voices, kept a
    /// year after their last use). Never repeated after a timeout, so one
    /// click makes at most one voice.
    ///
    /// Metered at the TTS model's rates from the tokens Google reports, or
    /// from the sample's length when it reports none. Both are estimates:
    /// Google's pricing page does not list Voice Design (on 2026-10-05 one
    /// voice reported 219 input, 630 audio and 926 thinking tokens, about
    /// $0.014 at those rates).
    pub async fn create_voice(&self, design: &VoiceDesign) -> Result<CreatedVoice, LlmError> {
        let body = voice_design_body(design);
        let started = Instant::now();
        let response = self
            .send(
                Method::POST,
                "voices",
                &[],
                Some(&body),
                DESIGN_TIMEOUT,
                Retry::BeforeWork,
            )
            .await
            .map_err(|e| match e {
                LlmError::Timeout => LlmError::VoiceDesignTimeout,
                LlmError::Rejected { body, .. } => LlmError::VoiceNotCreated(body),
                other => other,
            })?;
        let created = designed_voice_from(&response);
        let sample = created.as_ref().ok().and_then(|c| c.sample.as_ref());
        let usage = design_usage(&self.tts_model, &response, sample, now_secs());
        tracing::info!(
            model = self.tts_model.as_str(),
            input = usage.input_tokens,
            output = usage.output_tokens,
            thinking = usage.thinking_tokens,
            micro_usd = usage.micro_usd,
            latency_ms = started.elapsed().as_millis() as u64,
            "Gemini voice design (cost estimated at TTS rates)"
        );
        self.add_usage(&usage);
        created
    }

    /// Sends a request, meters the response, then checks that it finished.
    async fn billed(&self, model: &str, body: Value, timeout: Duration) -> Result<Value, LlmError> {
        let started = Instant::now();
        let response = self.call(&body, timeout).await?;
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

    /// One interaction (`POST /interactions`).
    async fn call(&self, body: &Value, timeout: Duration) -> Result<Value, LlmError> {
        self.send(
            Method::POST,
            "interactions",
            &[],
            Some(body),
            timeout,
            Retry::Always,
        )
        .await
    }

    /// Any request under `API_ROOT`: `path` is relative ("interactions",
    /// "voices"). Retries what `retry` allows (429, 503, 504 and timeouts)
    /// with exponential backoff (a 429's `retryDelay` when Google gives one);
    /// a refused key is marked in `secrets`. A request to create a voice that
    /// Google refuses for the project's voice limit is `VoiceLimit`, at once
    /// when the refusal says so, or when it is still "resource exhausted"
    /// after the retries. An empty success body is `Value::Null`.
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        timeout: Duration,
        retry: Retry,
    ) -> Result<Value, LlmError> {
        let url = format!("{API_ROOT}/{path}");
        let creates_voice = method == Method::POST && path == "voices";
        let mut backoff_ms = FIRST_BACKOFF_MS;
        let mut exhausted = false;
        for attempt in 0..=MAX_RETRIES {
            let mut request = self
                .http
                .request(method.clone(), &url)
                .header("x-goog-api-key", &self.api_key)
                .timeout(timeout);
            if !query.is_empty() {
                request = request.query(query);
            }
            if let Some(body) = body {
                request = request.json(body);
            }
            let sent = Instant::now();
            let (retry_reason, asked_wait) = match request.send().await {
                Ok(response) if response.status().is_success() => {
                    super::super::secrets::mark_accepted(&self.api_key);
                    let text = response
                        .text()
                        .await
                        .map_err(|e| LlmError::Malformed(e.to_string()))?;
                    if text.trim().is_empty() {
                        return Ok(Value::Null);
                    }
                    return serde_json::from_str(&text)
                        .map_err(|e| LlmError::Malformed(e.to_string()));
                }
                Ok(response) if matches!(response.status().as_u16(), 429 | 503 | 504) => {
                    let status = response.status().as_u16();
                    let text = response.text().await.unwrap_or_default();
                    if creates_voice && voice_limit(status, &text) {
                        return Err(LlmError::VoiceLimit);
                    }
                    // A 503 that took a while may come after the work was
                    // done (seen once, after 62 s, on 2026-10-05).
                    if retry == Retry::BeforeWork
                        && (status == 504 || (status == 503 && sent.elapsed() > QUICK_REFUSAL))
                    {
                        return Err(LlmError::Timeout);
                    }
                    exhausted = status == 429 && resource_exhausted(&text);
                    let asked = if status == 429 {
                        retry_delay(&text)
                    } else {
                        None
                    };
                    (LlmError::Busy, asked)
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let text = response.text().await.unwrap_or_default();
                    return Err(match refusal(status, &text, path, body) {
                        LlmError::KeyRejected => LlmError::ActiveKeyRejected(
                            super::super::secrets::mark_rejected(&self.api_key),
                        ),
                        other => other,
                    });
                }
                Err(e) if e.is_timeout() && retry == Retry::BeforeWork => {
                    return Err(LlmError::Timeout);
                }
                Err(e) if e.is_timeout() => (LlmError::Timeout, None),
                Err(e) => return Err(LlmError::Network(e.to_string())),
            };
            if attempt == MAX_RETRIES {
                return Err(if creates_voice && exhausted {
                    LlmError::VoiceLimit
                } else {
                    retry_reason
                });
            }
            let wait = asked_wait.unwrap_or(Duration::from_millis(backoff_ms));
            tracing::warn!(
                path,
                attempt,
                wait_ms = wait.as_millis() as u64,
                "Gemini call will be retried: {retry_reason}"
            );
            tokio::time::sleep(wait).await;
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

/// A speech request: one `text` item per turn, each carrying its delivery
/// style (when it has one) and, with two voices, its speaker as
/// `speech_metadata`; an item with neither has no annotation. Raw 24 kHz PCM
/// is requested instead of the default WAV. Designed voices only read alone,
/// so a two-voice request naming one is refused before it is sent.
fn speech_body(model: &str, request: &SpeechRequest) -> Result<Value, LlmError> {
    if request.turns.is_empty() {
        return Err(LlmError::Malformed("nothing to read aloud".into()));
    }
    let speech_config = match request.voices.as_slice() {
        [] => return Err(LlmError::Malformed("no voice assigned".into())),
        [single] => json!([{ "voice": single.voice }]),
        [first, second] => {
            if [first, second]
                .iter()
                .any(|v| Voice::is_designed_id(&v.voice))
            {
                return Err(LlmError::Malformed(
                    "a designed voice is read one speaker at a time, never in a two-voice request"
                        .into(),
                ));
            }
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
        let mut metadata = serde_json::Map::new();
        if !turn.style.trim().is_empty() {
            metadata.insert("style".into(), json!(turn.style));
        }
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
            metadata.insert("speaker".into(), json!(wire_speaker(speaker)));
        }
        let mut item = json!({ "type": "text", "text": turn.text });
        if !metadata.is_empty() {
            metadata.insert("type".into(), json!("speech_metadata"));
            item["annotations"] = json!([metadata]);
        }
        content.push(item);
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
    // The Voices API names it "models/gemini-3.8-flash-tts".
    let served = response
        .get("model")
        .and_then(Value::as_str)
        .map(|m| m.trim_start_matches("models/"));
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

/// The `error` object of a non-2xx body, `{"error": {"code": "...",
/// "message": "..."}}`, which Google sometimes wraps in a one-element array.
fn error_object(body: &str) -> Option<Value> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    parsed.get(0).unwrap_or(&parsed).get("error").cloned()
}

/// A non-2xx response as a readable error.
fn error_of(status: u16, body: &str) -> LlmError {
    let error = error_object(body);
    let error = error.as_ref();
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
    if rejects_key(status, message, error) {
        return LlmError::KeyRejected;
    }
    LlmError::Rejected {
        status,
        body: message.chars().take(500).collect(),
    }
}

/// A refusal of a request to `path` with `request` as its body. A refusal
/// about a voice is told apart first: Google's voice errors can mention the
/// "API key's project", and taking one for a rejected key would send the
/// teacher to replace a key that works.
fn refusal(status: u16, body: &str, path: &str, request: Option<&Value>) -> LlmError {
    let error = error_of(status, body);
    if matches!(error, LlmError::Blocked(_)) {
        return error;
    }
    // Voice Design (the only request with a body to "voices") refused for
    // the project's voice count.
    if path == "voices" && request.is_some() && status != 401 && voice_limit(status, body) {
        return LlmError::VoiceLimit;
    }
    voice_refusal(status, body, path, request).unwrap_or(error)
}

/// Whether a refusal says the project cannot hold another designed voice:
/// the message is about voices and a limit, or a quota that names voices
/// ran out.
fn voice_limit(status: u16, body: &str) -> bool {
    let Some(error) = error_object(body) else {
        return false;
    };
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let limit_words = ["limit", "maximum", "quota", "exceed", "too many"];
    let says_so = message.contains("voice") && limit_words.iter().any(|w| message.contains(w));
    let quota_names_voices = matches!(status, 429 | 400 | 403)
        && error
            .get("details")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|detail| {
                let violations = detail
                    .get("violations")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                std::iter::once(detail.clone()).chain(violations)
            })
            .any(|detail| {
                ["quotaMetric", "quotaId", "reason", "subject"]
                    .iter()
                    .filter_map(|field| detail.get(*field).and_then(Value::as_str))
                    .any(|value| value.to_ascii_lowercase().contains("voice"))
            });
    says_so || quota_names_voices
}

/// A 429 whose status is `RESOURCE_EXHAUSTED` (a quota, not a busy server).
fn resource_exhausted(body: &str) -> bool {
    error_object(body)
        .and_then(|e| e.get("status").and_then(Value::as_str).map(str::to_string))
        .is_some_and(|status| status == "RESOURCE_EXHAUSTED")
}

/// `voices/{id}` for an id that is safe in a path; nothing is built from
/// anything else.
fn voice_path(id: &str) -> Result<String, LlmError> {
    Voice::check_id(id).map_err(|e| LlmError::NotAVoiceId(e.to_string()))?;
    Ok(format!("voices/{id}"))
}

/// A 403 or 404 for `voices/{id}`, whatever its wording, is a voice this key
/// cannot use. Key refusals stay key refusals.
fn unknown_voice(error: LlmError, id: &str) -> LlmError {
    match error {
        LlmError::Rejected {
            status: 403 | 404, ..
        } => LlmError::UnknownVoice(id.to_string()),
        other => other,
    }
}

/// Google's gender word.
fn wire_gender(gender: Gender) -> &'static str {
    match gender {
        Gender::Female => "female",
        Gender::Male => "male",
    }
}

/// A Voice Design request: a prompted voice, stored in the project. There
/// is no `voice.model`: without one, Google made the voice for
/// gemini-3.8-flash-tts on 2026-10-05 (probe E9), the shape this sends.
fn voice_design_body(design: &VoiceDesign) -> Value {
    json!({
        "store": true,
        "voice": {
            "type": "prompted",
            "display_name": design.display_name,
            "gender": wire_gender(design.gender),
            "language_code": design.language_code,
            "prompted": { "input": design.description },
        },
    })
}

/// The Voice object Voice Design answers with (the shape seen on
/// 2026-10-05: `id`, `display_name`, `gender`, `language_code`, `model`,
/// `expire_time`, `prompted`, `sample_audio`, `usage`). The id must be a
/// voice id; the sample is optional.
fn designed_voice_from(response: &Value) -> Result<CreatedVoice, LlmError> {
    let listed = catalog_voice(response)
        .ok_or_else(|| LlmError::Malformed("the new voice came back without an id".into()))?;
    Voice::check_id(&listed.id).map_err(|_| {
        LlmError::Malformed("the new voice came back with an id that is not a voice id".into())
    })?;
    Ok(CreatedVoice {
        id: listed.id,
        display_name: listed.display_name,
        gender: listed.gender,
        language_code: listed.language_code,
        sample: sample_of(response),
    })
}

/// A voice's `sample_audio` (base64 WAV, or raw L16 with its rate). One
/// that is missing or does not decode is `None`: the voice is usable
/// without it.
fn sample_of(voice: &Value) -> Option<Pcm16> {
    let sample = voice.get("sample_audio")?;
    let data = sample.get("data").and_then(Value::as_str)?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            if bytes.starts_with(b"RIFF") {
                Pcm16::from_wav(&bytes)
            } else {
                let rate = sample
                    .get("sample_rate")
                    .and_then(Value::as_u64)
                    .map_or(SAMPLE_RATE, |rate| rate as u32);
                Ok(Pcm16::from_le_bytes(&bytes, rate))
            }
        });
    match decoded {
        Ok(pcm) if pcm.duration_ms() > 0 => Some(pcm),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("a voice sample from Google did not decode: {e}");
            None
        }
    }
}

/// What one Voice Design request cost, as far as can be told: the reported
/// tokens at the TTS model's rates or, with no output tokens reported, the
/// sample's length at `AUDIO_TOKENS_PER_SECOND`. An ESTIMATE either way:
/// Google prices Voice Design nowhere in its tables.
fn design_usage(tts_model: &str, response: &Value, sample: Option<&Pcm16>, at_secs: i64) -> Usage {
    let mut usage = priced_usage(tts_model, response, at_secs);
    if usage.output_tokens == 0
        && let Some(sample) = sample
    {
        let output_tokens = u64::from(sample.duration_ms()) * AUDIO_TOKENS_PER_SECOND / 1_000;
        usage.output_tokens = output_tokens;
        usage.micro_usd += rates_for(tts_model, at_secs)
            .map(|rates| cost_micro_usd(rates, 0, 0, output_tokens, 0))
            .unwrap_or(0);
    }
    usage
}

/// `UnknownVoice` for a 400, 403 or 404 to a request that names voices (a
/// speech request, or `voices/{id}`) when the error is about a voice, or is a
/// 404 for `voices/{id}`. It names the voices the message names, else all of
/// the request's. 401 is always the key.
fn voice_refusal(status: u16, body: &str, path: &str, request: Option<&Value>) -> Option<LlmError> {
    if !matches!(status, 400 | 403 | 404) {
        return None;
    }
    let voice_path = path.strip_prefix("voices/").filter(|id| !id.is_empty());
    let mut ids = request.map(speech_voices).unwrap_or_default();
    ids.extend(voice_path.map(str::to_string));
    if ids.is_empty() {
        return None;
    }
    let message = error_object(body)
        .and_then(|e| e.get("message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.to_string())
        .to_ascii_lowercase();
    if !message.contains("voice") && !(status == 404 && voice_path.is_some()) {
        return None;
    }
    let named: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| message.contains(&id.to_ascii_lowercase()))
        .collect();
    let shown = if named.is_empty() {
        ids.join(", ")
    } else {
        named.join(", ")
    };
    Some(LlmError::UnknownVoice(shown))
}

/// The voice ids a speech request body names, in `speech_config` order.
fn speech_voices(request: &Value) -> Vec<String> {
    let Some(config) = request.pointer("/generation_config/speech_config") else {
        return Vec::new();
    };
    let entries = config
        .as_array()
        .or_else(|| config.get("speakers").and_then(Value::as_array));
    entries
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("voice").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// The wait a 429 asks for (`google.rpc.RetryInfo`, `"retryDelay": "30s"`),
/// capped at `MAX_RETRY_DELAY`.
fn retry_delay(body: &str) -> Option<Duration> {
    let error = error_object(body)?;
    let delay = error
        .get("details")?
        .as_array()?
        .iter()
        .find_map(|detail| detail.get("retryDelay").and_then(Value::as_str))?;
    let seconds: f64 = delay.trim().strip_suffix('s')?.trim().parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0)
        .then(|| Duration::from_secs_f64(seconds.min(MAX_RETRY_DELAY.as_secs_f64())))
}

/// One catalogue entry. Google's fields come and go, so each is read on its
/// own (a number becomes its text, anything missing is empty); an entry
/// without an id (`id`, or `name` as "voices/{id}") is skipped.
fn catalog_voice(value: &Value) -> Option<CatalogVoice> {
    let field = |name: &str| match value.get(name) {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    };
    let id = Some(field("id")).filter(|id| !id.is_empty()).or_else(|| {
        let name = field("name");
        let id = name.strip_prefix("voices/").unwrap_or(&name);
        (!id.is_empty()).then(|| id.to_string())
    })?;
    // A designed voice has no description; its design prompt describes it.
    let description = Some(field("description"))
        .filter(|d| !d.is_empty())
        .or_else(|| {
            value
                .pointer("/prompted/input")
                .and_then(Value::as_str)
                .map(|input| input.trim().to_string())
        })
        .unwrap_or_default();
    Some(CatalogVoice {
        id,
        display_name: field("display_name"),
        description,
        gender: field("gender"),
        accent: field("accent"),
        language_code: field("language_code"),
        region_code: field("region_code"),
        pitch: field("pitch"),
        persona: field("persona"),
        context: field("context"),
        voice_type: field("type"),
    })
}

/// Whether a refusal is about the API key: always for 401; for 400 and 403
/// only when Google says so (`API_KEY_INVALID`, "API key not valid", "Please
/// use API Key"), since both also mean a bad request or a denied model.
fn rejects_key(status: u16, message: &str, error: Option<&Value>) -> bool {
    let mut reasons = error
        .and_then(|e| e.get("details"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|detail| detail.get("reason").and_then(Value::as_str));
    let about_key = message.to_ascii_lowercase().contains("api key")
        || reasons.any(|reason| reason.starts_with("API_KEY_"));
    status == 401 || (matches!(status, 400 | 403) && about_key)
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
        styled(speaker, text, "calm")
    }

    fn styled(speaker: Option<&str>, text: &str, style: &str) -> SpeechTurn {
        SpeechTurn {
            speaker: speaker.map(Into::into),
            text: text.into(),
            style: style.into(),
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
            turns: vec![styled(None, "Part one.", "slow")],
            voices: vec![voice("Announcer", "Charon")],
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
        };
        assert!(speech_body("m", &request).is_err(), "unknown speaker");
        request.voices.push(voice("Speaker C", "Fenrir"));
        assert!(speech_body("m", &request).is_err(), "three voices");
        request.voices.clear();
        assert!(speech_body("m", &request).is_err(), "no voice");
    }

    #[test]
    fn each_turn_carries_its_own_style() {
        let request = SpeechRequest {
            turns: vec![
                styled(Some("Speaker A"), "Welcome.", "polite and helpful"),
                styled(Some("Speaker B"), "Thanks.", "relaxed and conversational"),
                styled(Some("Speaker A"), "Sit down.", "polite and helpful"),
            ],
            voices: vec![
                voice("Speaker A", "en-gb-advisor-1"),
                voice("Speaker B", "en-gb-assistant-2"),
            ],
        };
        let body = speech_body("m", &request).unwrap();
        let content = &body["input"][0]["content"];
        assert_eq!(
            content[1]["annotations"],
            json!([{ "type": "speech_metadata", "style": "relaxed and conversational", "speaker": "SpeakerB" }])
        );
        assert_eq!(
            content[2]["annotations"][0]["style"],
            json!("polite and helpful")
        );
        assert!(body.get("style").is_none());
    }

    #[test]
    fn an_empty_style_sends_no_annotation() {
        let solo = SpeechRequest {
            turns: vec![styled(None, "Part one.", " ")],
            voices: vec![voice("Announcer", "en-gb-tutor-9")],
        };
        let body = speech_body("m", &solo).unwrap();
        assert_eq!(
            body["input"][0]["content"][0],
            json!({ "type": "text", "text": "Part one." })
        );
        // With two voices the item still names its speaker, without a style.
        let pair = SpeechRequest {
            turns: vec![
                styled(Some("Speaker A"), "Hi.", ""),
                styled(Some("Speaker B"), "Hello.", "warm"),
            ],
            voices: vec![voice("Speaker A", "a"), voice("Speaker B", "b")],
        };
        let content = &speech_body("m", &pair).unwrap()["input"][0]["content"];
        assert_eq!(
            content[0]["annotations"],
            json!([{ "type": "speech_metadata", "speaker": "SpeakerA" }])
        );
    }

    #[test]
    fn custom_voices_refused_in_two_voice_requests() {
        for designed in ["voice_kpd3e297369r", "voicekey_abc123"] {
            let request = SpeechRequest {
                turns: vec![
                    turn(Some("Speaker A"), "Hi."),
                    turn(Some("Speaker B"), "Hey."),
                ],
                voices: vec![
                    voice("Speaker A", designed),
                    voice("Speaker B", "en-gb-advisor-1"),
                ],
            };
            assert!(matches!(
                speech_body("m", &request),
                Err(LlmError::Malformed(message)) if message.contains("designed voice")
            ));
        }
        // Alone, a designed voice is fine.
        let alone = SpeechRequest {
            turns: vec![turn(None, "Hi.")],
            voices: vec![voice("Speaker A", "voice_kpd3e297369r")],
        };
        assert!(speech_body("m", &alone).is_ok());
    }

    #[test]
    fn voice_list_query_pages_and_filters() {
        let query = VoiceQuery {
            language_code: Some("en-GB".into()),
            gender: Some("female".into()),
            voice_type: Some("prebuilt".into()),
            accent: Some(" ".into()),
            page_size: None,
        };
        assert_eq!(
            query.pairs(None),
            vec![
                ("page_size", "1000".to_string()),
                ("language_code", "en-GB".to_string()),
                ("gender", "female".to_string()),
                ("type", "prebuilt".to_string()),
            ]
        );
        let next = VoiceQuery {
            page_size: Some(50),
            ..VoiceQuery::default()
        }
        .pairs(Some("ERh6R-3lB44"));
        assert_eq!(
            next,
            vec![
                ("page_size", "50".to_string()),
                ("page_token", "ERh6R-3lB44".to_string()),
            ]
        );
    }

    #[test]
    fn catalog_voice_tolerates_missing_fields() {
        // As listed by GET /v1beta/voices on 2026-10-05.
        let library = json!({
            "id": "en-gb-advisor-1", "type": "prebuilt", "display_name": "Authoritative Advisor 1",
            "language_code": "en-GB", "region_code": "GB", "accent": "Winchester English",
            "persona": "High-Trust Advisor / Authoritative Advisor (Lawyer)", "context": "Enterprise Agent",
            "gender": "female", "pitch": "medium", "description": "Speaks with a Winchester English accent.",
        });
        let voice = catalog_voice(&library).unwrap();
        assert_eq!(voice.id, "en-gb-advisor-1");
        assert_eq!(
            (
                voice.gender.as_str(),
                voice.accent.as_str(),
                voice.voice_type.as_str()
            ),
            ("female", "Winchester English", "prebuilt")
        );
        // A designed voice has no accent, pitch or persona.
        let designed = json!({
            "id": "voice_60zf03beui2x", "model": "models/gemini-3.8-flash-tts", "type": "prompted",
            "display_name": "IELTS S3 Announcer", "prompted": { "input": "A woman in her forties." },
            "language_code": "en-GB", "gender": "female",
        });
        let voice = catalog_voice(&designed).unwrap();
        assert_eq!((voice.accent.as_str(), voice.pitch.as_str()), ("", ""));
        let named =
            catalog_voice(&json!({ "name": "voices/en-ie-advisor-2", "pitch": 3 })).unwrap();
        assert_eq!(
            (named.id.as_str(), named.pitch.as_str()),
            ("en-ie-advisor-2", "3")
        );
        assert_eq!(catalog_voice(&json!({ "display_name": "No id" })), None);
    }

    #[test]
    fn voice_errors_are_not_key_rejections() {
        let speech = speech_body(
            "m",
            &SpeechRequest {
                turns: vec![turn(None, "Hello.")],
                voices: vec![voice("Speaker A", "voice_doesnotexist0000")],
            },
        )
        .unwrap();
        // As returned on 2026-10-05 by POST /v1beta/interactions for an unknown designed voice.
        let unknown = r#"{"error":{"message":"The voice was not found or the caller does not have permission to access it.","code":"not_found"}}"#;
        assert!(matches!(
            refusal(404, unknown, "interactions", Some(&speech)),
            LlmError::UnknownVoice(id) if id == "voice_doesnotexist0000"
        ));
        // A voice error that mentions the key's project is still about the voice.
        let foreign = r#"{"error":{"code":403,"message":"Voice voice_doesnotexist0000 is not found in this API key's project.","status":"PERMISSION_DENIED"}}"#;
        assert!(matches!(error_of(403, foreign), LlmError::KeyRejected));
        assert!(matches!(
            refusal(403, foreign, "interactions", Some(&speech)),
            LlmError::UnknownVoice(id) if id == "voice_doesnotexist0000"
        ));
        // GET voices/{id} of a voice this key cannot see.
        let missing = r#"{"error":{"code":404,"message":"The voice was not found or the caller does not have permission to access it.","status":"NOT_FOUND"}}"#;
        assert!(matches!(
            refusal(404, missing, "voices/en-gb-advisor-1", None),
            LlmError::UnknownVoice(id) if id == "en-gb-advisor-1"
        ));
        // Leaked, expired and missing keys stay key rejections, in speech
        // requests and on the voice catalogue alike.
        let leaked = r#"{"error":{"code":403,"message":"Your API key was reported as leaked. Please use another API key.","status":"PERMISSION_DENIED"}}"#;
        let expired = r#"[{"error":{"code":400,"message":"API key expired. Please renew the API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com"}]}}]"#;
        for (status, body) in [(403, leaked), (400, expired)] {
            for path in ["interactions", "voices", "voices/en-gb-advisor-1"] {
                assert!(
                    matches!(
                        refusal(status, body, path, Some(&speech)),
                        LlmError::KeyRejected
                    ),
                    "{path}: {body}"
                );
            }
        }
        assert!(matches!(
            refusal(401, unknown, "interactions", Some(&speech)),
            LlmError::KeyRejected
        ));
        // A text request that mentions a voice is an ordinary refusal.
        assert!(matches!(
            refusal(
                400,
                r#"{"error":{"message":"bad voice field"}}"#,
                "interactions",
                Some(&json!({ "input": "x" }))
            ),
            LlmError::Rejected { status: 400, .. }
        ));
    }

    fn teacher_design() -> VoiceDesign {
        VoiceDesign {
            display_name: "probe 2026-10 British teacher".into(),
            gender: Gender::Female,
            language_code: "en-GB".into(),
            description: "A woman in her forties with a warm, clear Southern British accent, an experienced teacher speaking at a steady pace.".into(),
        }
    }

    #[test]
    fn voice_design_body_is_stored_prompted_and_has_no_model() {
        let body = voice_design_body(&teacher_design());
        // As sent by the probe that made voice_kwq20yi2gjin on 2026-10-05.
        assert_eq!(
            body,
            json!({
                "store": true,
                "voice": {
                    "type": "prompted",
                    "display_name": "probe 2026-10 British teacher",
                    "gender": "female",
                    "language_code": "en-GB",
                    "prompted": { "input": "A woman in her forties with a warm, clear Southern British accent, an experienced teacher speaking at a steady pace." },
                },
            })
        );
        assert!(body["voice"].get("model").is_none());
        assert!(body.get("model").is_none());
        let male = VoiceDesign {
            gender: Gender::Male,
            ..teacher_design()
        };
        assert_eq!(voice_design_body(&male)["voice"]["gender"], json!("male"));
    }

    /// The response of `POST /v1beta/voices` on 2026-10-05 (probe E9), with
    /// its 20 s sample replaced by `SAMPLE` (a short WAV made by the test).
    const E9_RESPONSE: &str = r#"{
        "id": "voice_kwq20yi2gjin",
        "model": "models/gemini-3.8-flash-tts",
        "type": "prompted",
        "expire_time": "2027-10-05T10:22:19.809240699Z",
        "display_name": "probe 2026-10 British teacher",
        "prompted": {"input": "A woman in her forties with a warm, clear Southern British accent, an experienced teacher speaking at a steady pace."},
        "language_code": "en-GB",
        "gender": "female",
        "usage": {
            "total_tokens": 849, "total_input_tokens": 219,
            "input_tokens_by_modality": [{"modality": "text", "tokens": 69}],
            "total_cached_tokens": 0, "total_output_tokens": 630,
            "output_tokens_by_modality": [{"modality": "audio", "tokens": 630}],
            "total_tool_use_tokens": 0, "total_thought_tokens": 926, "raw_prompt_token": 382
        },
        "sample_audio": {"mime_type": "audio/wav", "data": "SAMPLE"}
    }"#;

    fn e9_response(sample_ms: u32) -> Value {
        let wav = Pcm16::tone(220.0, sample_ms, 0.3, SAMPLE_RATE).to_wav();
        let data = base64::engine::general_purpose::STANDARD.encode(wav);
        serde_json::from_str(&E9_RESPONSE.replace("SAMPLE", &data)).unwrap()
    }

    #[test]
    fn designed_voice_response_is_parsed() {
        let response = e9_response(1_500);
        let created = designed_voice_from(&response).unwrap();
        assert_eq!(created.id, "voice_kwq20yi2gjin");
        assert_eq!(created.display_name, "probe 2026-10 British teacher");
        assert_eq!(
            (created.gender.as_str(), created.language_code.as_str()),
            ("female", "en-GB")
        );
        let sample = created.sample.unwrap();
        assert_eq!(
            (sample.sample_rate, sample.duration_ms()),
            (SAMPLE_RATE, 1_500)
        );
        // Listed, the same voice describes itself with its design prompt.
        let listed = catalog_voice(&response).unwrap();
        assert!(listed.description.starts_with("A woman in her forties"));
        assert_eq!(listed.voice_type, "prompted");

        // Priced at TTS rates from the reported tokens (an estimate):
        // 219 x $0.50 + (630 + 926) x $9.00 per million at the 2026 rates,
        // 14,113.5 µUSD, as the probe's ledger recorded.
        let usage = design_usage(
            "gemini-3.8-flash-tts",
            &response,
            sample_of(&response).as_ref(),
            0,
        );
        assert_eq!(
            (
                usage.requests,
                usage.input_tokens,
                usage.output_tokens,
                usage.thinking_tokens
            ),
            (1, 219, 630, 926)
        );
        assert_eq!(usage.micro_usd, 14_114);
        assert_eq!(usage.unpriced, 0);

        // Without token counts, the sample's length is billed instead.
        let mut bare = e9_response(2_000);
        bare.as_object_mut().unwrap().remove("usage");
        let sample = sample_of(&bare).unwrap();
        let usage = design_usage("gemini-3.8-flash-tts", &bare, Some(&sample), 0);
        assert_eq!(usage.output_tokens, 64);
        assert_eq!(usage.micro_usd, 64 * 9);

        // A missing or broken sample leaves the voice usable; a missing or
        // unsafe id does not.
        let mut no_sample = e9_response(10);
        no_sample["sample_audio"]["data"] = json!("not base64!");
        assert_eq!(designed_voice_from(&no_sample).unwrap().sample, None);
        no_sample.as_object_mut().unwrap().remove("sample_audio");
        assert_eq!(designed_voice_from(&no_sample).unwrap().sample, None);
        let mut unsafe_id = e9_response(10);
        unsafe_id["id"] = json!("../voices");
        assert!(matches!(
            designed_voice_from(&unsafe_id),
            Err(LlmError::Malformed(_))
        ));
        assert!(designed_voice_from(&json!({ "display_name": "x" })).is_err());
    }

    #[test]
    fn voice_errors_are_readable() {
        // GET or DELETE voices/{id} that this key cannot see, whatever the wording.
        let not_found = r#"{"error":{"message":"The voice was not found or the caller does not have permission to access it.","code":"not_found"}}"#;
        let denied =
            r#"{"error":{"code":403,"message":"Permission denied.","status":"PERMISSION_DENIED"}}"#;
        for (status, body) in [(404, not_found), (403, denied)] {
            let error = unknown_voice(
                refusal(status, body, "voices/voice_60zf03beui2x", None),
                "voice_60zf03beui2x",
            );
            assert!(
                matches!(&error, LlmError::UnknownVoice(id) if id == "voice_60zf03beui2x"),
                "{status}: {error:?}"
            );
        }
        // A refused key stays a refused key there too.
        let leaked = r#"{"error":{"code":403,"message":"Your API key was reported as leaked. Please use another API key.","status":"PERMISSION_DENIED"}}"#;
        assert!(matches!(
            unknown_voice(refusal(403, leaked, "voices/voice_abc", None), "voice_abc"),
            LlmError::KeyRejected
        ));
        // Nothing that is not a voice id becomes a path.
        for bad in ["", "../models", "voice_a/b", "voice a"] {
            assert!(
                matches!(voice_path(bad), Err(LlmError::NotAVoiceId(_))),
                "{bad}"
            );
        }
        assert_eq!(
            voice_path("voice_kwq20yi2gjin").unwrap(),
            "voices/voice_kwq20yi2gjin"
        );

        // The project's voice limit, said outright or as a quota on voices.
        let design = Some(voice_design_body(&teacher_design()));
        let full = r#"{"error":{"code":400,"message":"The project has reached the maximum number of voices (200).","status":"FAILED_PRECONDITION"}}"#;
        let quota = r#"{"error":{"code":429,"message":"Resource has been exhausted.","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaMetric":"generativelanguage.googleapis.com/stored_voices","quotaId":"StoredVoicesPerProject"}]}]}}"#;
        assert!(matches!(
            refusal(400, full, "voices", design.as_ref()),
            LlmError::VoiceLimit
        ));
        assert!(voice_limit(429, quota) && resource_exhausted(quota));
        let busy = r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota).","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaMetric":"generativelanguage.googleapis.com/generate_requests_per_model"}]}]}}"#;
        assert!(!voice_limit(429, busy) && resource_exhausted(busy));
        // Other refusals of a design are about the request, said without a status code.
        let bad =
            r#"{"error":{"code":400,"message":"Invalid prompt","status":"INVALID_ARGUMENT"}}"#;
        assert!(matches!(
            refusal(400, bad, "voices", design.as_ref()),
            LlmError::Rejected { status: 400, .. }
        ));

        // Every message a teacher may see says what to do and shows no HTTP code.
        let messages = [
            LlmError::UnknownVoice("voice_60zf03beui2x".into()).to_string(),
            LlmError::VoiceLimit.to_string(),
            LlmError::VoiceNotCreated("Invalid prompt".into()).to_string(),
            LlmError::VoiceDesignTimeout.to_string(),
        ];
        assert!(messages[1].contains("200 designed voices"));
        for message in messages
            .iter()
            .chain([&voice_path("../x").unwrap_err().to_string()])
        {
            for code in ["400", "403", "404", "429"] {
                assert!(!message.contains(code), "{message}");
            }
        }
        assert!(messages.iter().all(|m| m.ends_with('.')), "{messages:?}");
    }

    #[test]
    fn retry_delay_is_read_from_429_details() {
        let busy = r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota).","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaMetric":"generativelanguage.googleapis.com/generate_requests_per_model"}]},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"12s"}]}}"#;
        assert_eq!(retry_delay(busy), Some(Duration::from_secs(12)));
        let wrapped = r#"[{"error":{"details":[{"retryDelay":"1.5s"}]}}]"#;
        assert_eq!(retry_delay(wrapped), Some(Duration::from_millis(1_500)));
        let long = r#"{"error":{"details":[{"retryDelay":"3600s"}]}}"#;
        assert_eq!(retry_delay(long), Some(MAX_RETRY_DELAY));
        for none in [
            r#"{"error":{"message":"busy"}}"#,
            r#"{"error":{"details":[{"retryDelay":"soon"}]}}"#,
            "not json",
        ] {
            assert_eq!(retry_delay(none), None, "{none}");
        }
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

    #[test]
    fn refused_keys_are_told_apart_from_bad_requests() {
        // As returned on 2026-10-05 by POST /v1beta/interactions with a made-up key.
        let invalid = r#"[{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com"}]}}]"#;
        assert!(matches!(error_of(400, invalid), LlmError::KeyRejected));
        // ... and with no key at all.
        let missing = r#"[{"error":{"code":403,"message":"Method doesn't allow unregistered callers (callers without established identity). Please use API Key or other form of API consumer identity to call this API.","status":"PERMISSION_DENIED"}}]"#;
        assert!(matches!(error_of(403, missing), LlmError::KeyRejected));
        let reason_only =
            r#"{"error":{"message":"denied","details":[{"reason":"API_KEY_SERVICE_BLOCKED"}]}}"#;
        assert!(matches!(error_of(403, reason_only), LlmError::KeyRejected));
        assert!(matches!(error_of(401, "{}"), LlmError::KeyRejected));
        // The array wrapper no longer hides the message.
        assert!(matches!(
            error_of(400, r#"[{"error":{"code":400,"message":"bad field"}}]"#),
            LlmError::Rejected { status: 400, body } if body == "bad field"
        ));
        assert!(matches!(
            error_of(403, r#"{"error":{"message":"model not available"}}"#),
            LlmError::Rejected { status: 403, .. }
        ));
    }

    /// Calls the real API with the key from the environment (process, then
    /// Windows): text, JSON, the built-in announcer and a British dialogue on
    /// two library voices. Costs about $0.02. Run on demand:
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
        let announcer = super::super::super::tts::voices::VoiceCatalog::builtin()
            .announcer()
            .id
            .clone();
        let (before, started) = (client.usage(), Instant::now());
        let single = client
            .synthesize(&SpeechRequest {
                turns: vec![styled(
                    None,
                    "Part one. You will hear a conversation between a receptionist and a caller.",
                    "slow and clear, like an exam announcer",
                )],
                voices: vec![voice("Announcer", &announcer)],
            })
            .await
            .unwrap();
        std::fs::write(dir.join("ielts-probe-single.wav"), single.to_wav()).unwrap();
        report(
            &format!("tts announcer {announcer} ({tts_model})"),
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
        let style = |speaker: &str| match speaker {
            "Speaker A" => "polite and helpful, clear, at a steady exam pace",
            _ => "relaxed and conversational, clear, at a steady exam pace",
        };
        let pair = client
            .synthesize(&SpeechRequest {
                turns: dialogue
                    .iter()
                    .map(|(s, t)| styled(Some(s), t, style(s)))
                    .collect(),
                voices: vec![
                    voice("Speaker A", "en-gb-advisor-1"),
                    voice("Speaker B", "en-gb-assistant-2"),
                ],
            })
            .await
            .unwrap();
        std::fs::write(dir.join("ielts-probe-dialogue.wav"), pair.to_wav()).unwrap();
        report(
            "tts two library voices (en-gb-advisor-1 F, en-gb-assistant-2 M)",
            before,
            started,
            Some(pair.duration_ms()),
        );

        // The raw usage object of one speech response, audio data left out.
        let body = speech_body(
            &tts_model,
            &SpeechRequest {
                turns: vec![turn(None, "Thank you.")],
                voices: vec![voice("Announcer", &announcer)],
            },
        )
        .unwrap();
        let raw = client.call(&body, TTS_TIMEOUT).await.unwrap();
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
