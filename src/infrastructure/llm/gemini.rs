//! Google Gemini `generateContent` client for text, JSON and speech.
//!
//! The API key travels in the `x-goog-api-key` header, never in the URL, so it
//! cannot leak through logs or proxies.

use std::time::Duration;

use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::super::config::config;

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const TEXT_TIMEOUT: Duration = Duration::from_secs(90);
const TTS_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RETRIES: u32 = 3;
const FIRST_BACKOFF_MS: u64 = 1_000;

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error(
        "The AI service is not configured: set GEMINI_API_KEY in the environment or .env file."
    )]
    NoApiKey,
    #[error("The AI service is busy right now. Please try again in a minute.")]
    Busy,
    #[error("The AI service did not answer in time. Please try again.")]
    Timeout,
    #[error("The AI service rejected the request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("Network error while contacting the AI service: {0}")]
    Network(String),
    #[error("The AI service returned something unexpected: {0}")]
    Malformed(String),
}

/// A passage label bound to a prebuilt Gemini voice name.
#[derive(Debug, Clone)]
pub struct VoiceAssignment {
    pub label: String,
    pub voice: String,
}

#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
    text_model: String,
    tts_model: String,
}

impl GeminiClient {
    pub fn from_config() -> Result<Self, LlmError> {
        let cfg = config();
        let api_key = cfg.gemini_api_key.clone().ok_or(LlmError::NoApiKey)?;
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| LlmError::Network(e.to_string()))?;
        Ok(Self {
            http,
            api_key,
            text_model: cfg.text_model.clone(),
            tts_model: cfg.tts_model.clone(),
        })
    }

    /// Plain text completion.
    pub async fn generate_text(&self, prompt: &str) -> Result<String, LlmError> {
        let body = json!({ "contents": [{ "parts": [{ "text": prompt }] }] });
        let response = self.call(&self.text_model, body, TEXT_TIMEOUT).await?;
        first_text(&response)
    }

    /// JSON completion parsed into `T`. Markdown fences around the payload are tolerated.
    ///
    /// `responseMimeType` is the field documented in the `generateContent`
    /// REST reference; the newer docs describe a `responseFormat` object
    /// instead. If the server ever rejects the field, the prompt is retried
    /// without JSON mode and the fence-tolerant parser does the rest.
    pub async fn generate_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T, LlmError> {
        let json_prompt =
            format!("{prompt}\n\nRespond with a single JSON object and nothing else.");
        let body = json!({
            "contents": [{ "parts": [{ "text": json_prompt }] }],
            "generationConfig": { "responseMimeType": "application/json" }
        });
        let response = match self.call(&self.text_model, body, TEXT_TIMEOUT).await {
            Ok(response) => response,
            Err(LlmError::Rejected { status: 400, body }) if body.contains("responseMimeType") => {
                tracing::warn!(
                    "responseMimeType rejected by {}; retrying without JSON mode",
                    self.text_model
                );
                let plain = json!({ "contents": [{ "parts": [{ "text": json_prompt }] }] });
                self.call(&self.text_model, plain, TEXT_TIMEOUT).await?
            }
            Err(e) => return Err(e),
        };
        let text = first_text(&response)?;
        let payload = strip_fences(&text);
        serde_json::from_str(payload).map_err(|e| {
            LlmError::Malformed(format!(
                "{e}; payload starts with: {}",
                payload.chars().take(200).collect::<String>()
            ))
        })
    }

    /// Speech synthesis. Returns raw 16-bit little-endian PCM at 24 kHz, mono.
    /// One voice uses the single-speaker API; two voices the multi-speaker API.
    /// Gemini multi-speaker synthesis is limited to two voices per request;
    /// callers with more speakers must synthesise turn by turn.
    pub async fn synthesize(
        &self,
        text: &str,
        voices: &[VoiceAssignment],
    ) -> Result<Vec<u8>, LlmError> {
        let speech_config = match voices {
            [] => return Err(LlmError::Malformed("no voice assigned".into())),
            [single] => {
                json!({ "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": single.voice } } })
            }
            many => json!({
                "multiSpeakerVoiceConfig": {
                    "speakerVoiceConfigs": many.iter().map(|v| json!({
                        "speaker": v.label,
                        "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": v.voice } }
                    })).collect::<Vec<_>>()
                }
            }),
        };
        let body = json!({
            "contents": [{ "parts": [{ "text": text }] }],
            "generationConfig": { "responseModalities": ["AUDIO"], "speechConfig": speech_config }
        });
        let response = self.call(&self.tts_model, body, TTS_TIMEOUT).await?;
        let data = response
            .pointer("/candidates/0/content/parts/0/inlineData/data")
            .and_then(Value::as_str)
            .ok_or_else(|| LlmError::Malformed("no audio in the response".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| LlmError::Malformed(format!("audio is not valid base64: {e}")))
    }

    async fn call(&self, model: &str, body: Value, timeout: Duration) -> Result<Value, LlmError> {
        let url = format!("{BASE_URL}/{model}:generateContent");
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
                Ok(response) if matches!(response.status().as_u16(), 429 | 503) => LlmError::Busy,
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response.text().await.unwrap_or_default();
                    return Err(LlmError::Rejected {
                        status,
                        body: body.chars().take(500).collect(),
                    });
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

fn first_text(response: &Value) -> Result<String, LlmError> {
    response
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .ok_or_else(|| LlmError::Malformed("no text in the response".into()))
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

    #[test]
    fn fences_are_removed() {
        assert_eq!(strip_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fences("{\"a\":1}"), "{\"a\":1}");
    }
}
