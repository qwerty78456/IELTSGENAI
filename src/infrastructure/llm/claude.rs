//! Anthropic Claude client on the Messages API, for text only: topics,
//! scripts, questions and download-name summaries. Speech and voices stay
//! with Gemini.
//!
//! Every request goes to the hard-coded root `https://api.anthropic.com/v1`;
//! `ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`, which other tools set,
//! are never read. The key travels in the `x-api-key` header, marked
//! sensitive so no debug print shows it, never in the URL. The Messages API
//! keeps no conversation and has no per-request `store` switch: what
//! Anthropic retains follows the organisation's policy.
//!
//! A request carries the model, `max_tokens`, one fixed `system` line, the
//! prompt as one user turn and `output_config.effort`, the one thinking
//! control. It never carries `thinking`, sampling parameters, an assistant
//! prefill, `cache_control` or `stream`: 5.5 models refuse some of them and
//! the app needs none. Text-model requests may fall back
//! (`fallbacks: "default"`): one that Sonnet 5.5's safety classifier declines
//! runs again on the model Anthropic recommends, and every attempt is priced
//! at its own model's rates.
//!
//! Text server functions answer synchronously behind Cloudflare Tunnel, which
//! cuts a request at 125 s, so one call, retries included, has a budget of
//! 100 s. Nothing is repeated after a timeout or an error that came late:
//! Anthropic may have done, and billed, the work. Every billed response is
//! added to the usage meter before its `stop_reason` is checked, so a reply
//! that is cut off or refused is still counted.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::domain::{TextProvider, Usage};

use super::super::config::config;
use super::super::jobs::now_secs;
use super::super::secrets;
use super::gemini::{LlmError, strip_fences};
use super::pricing::{Rates, cost_micro_usd, rates_for};

const API_ROOT: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2023-06-01";
/// The beta that lets a request name `"fallbacks": "default"`.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// A connection that is not open by then was never sent, so never billed.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// A topic, script or question block, retries included: 25 s under the
/// tunnel's 125 s for the upload, the ledger write and the way back.
const TEXT_BUDGET: Duration = Duration::from_secs(100);
/// The least time a text attempt starts with: room for the longest script.
/// Measured 2026-10-14: the longest took 12.4 s with four written at once,
/// so this leaves about five times that.
const TEXT_MIN_ATTEMPT: Duration = Duration::from_secs(60);
/// A few words from the summary model come back in a second or two.
const BRIEF_BUDGET: Duration = Duration::from_secs(20);
const BRIEF_MIN_ATTEMPT: Duration = Duration::from_secs(5);
/// An error that takes longer than this may come after work that is billed,
/// so it is not repeated.
const FAST_FAIL: Duration = Duration::from_secs(10);
const MAX_RETRIES: u32 = 2;
const FIRST_BACKOFF_MS: u64 = 1_000;
/// Longest `retry-after` honoured; a longer one stops with `Busy`.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(20);
/// Cap on one text answer, thinking included. Every real answer fits in a
/// fraction of it; it only stops a runaway reply from running up the bill.
const MAX_TOKENS: u32 = 8_192;
/// Cap on a brief answer, thinking included; five words need about ten tokens.
const BRIEF_MAX_TOKENS: u32 = 1_024;
const BRIEF_EFFORT: &str = "low";
/// Prompts written for Gemini ask for bare output; Claude may otherwise open
/// with a sentence that would break `Passage::parse` or become a file name.
const SYSTEM: &str =
    "Output only what the user message asks for, with no preamble or closing remarks.";
/// The same suffix the Gemini client adds to a JSON prompt.
const JSON_SUFFIX: &str = "\n\nRespond with a single JSON object and nothing else.";
/// Anthropic's error messages are cut to this many characters.
const MESSAGE_CHARS: usize = 300;
/// `error.details.error_code` of the 429 that a tier's monthly spend cap
/// returns: nothing runs until the next month, so it is never repeated.
const SPEND_CAP: &str = "enforced_spend_limit_reached";
/// Shown when an error answer is not Anthropic's error envelope (a proxy's
/// page, say), so neither its text nor its status reaches the teacher.
const NO_REASON: &str = "its answer gave no reason";
const NO_PAYMENT: &str = "the account has a payment problem";
const TOO_LARGE: &str = "the request is too large";

/// A request with the text model (topics, scripts, questions) or with the
/// summary model (download-name summaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Text,
    Brief,
}

impl Call {
    fn max_tokens(self) -> u32 {
        match self {
            Call::Text => MAX_TOKENS,
            Call::Brief => BRIEF_MAX_TOKENS,
        }
    }

    /// Time for the request, retries included.
    fn budget(self) -> Duration {
        match self {
            Call::Text => TEXT_BUDGET,
            Call::Brief => BRIEF_BUDGET,
        }
    }

    /// The least time a retry may start with.
    fn min_attempt(self) -> Duration {
        match self {
            Call::Text => TEXT_MIN_ATTEMPT,
            Call::Brief => BRIEF_MIN_ATTEMPT,
        }
    }

    /// Whether a declined request falls back to another model. Haiku 5.5,
    /// the summary model, has no fallback.
    fn falls_back(self) -> bool {
        self == Call::Text
    }
}

#[derive(Clone)]
pub struct ClaudeClient {
    http: reqwest::Client,
    api_key: String,
    /// `api_key` as a header, marked sensitive.
    key_header: HeaderValue,
    text_model: String,
    summary_model: String,
    /// `output_config.effort` for text-model requests: low, medium or high.
    effort: String,
    /// Shared by clones, so the requests of one step add to one total.
    meter: Arc<Mutex<Usage>>,
}

impl ClaudeClient {
    /// Checks a key before it is kept: one model-list request, no tokens
    /// billed. It proves that the key works, not that the account may run
    /// requests. Never repeated; touches neither `secrets` nor a meter.
    pub async fn check_key(api_key: &str) -> Result<(), LlmError> {
        let key = key_header(api_key)?;
        let sent = request(&http_client()?, Method::GET, "models", &key, false)
            .query(&[("limit", "1")])
            .timeout(KEY_CHECK_TIMEOUT)
            .send()
            .await;
        match sent {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => {
                let status = response.status().as_u16();
                let headers = Headers::of(response.headers());
                let body = response.text().await.unwrap_or_default();
                key_check_outcome(status, &headers, &body)
            }
            Err(e) if e.is_connect() => Err(LlmError::Network(e.to_string())),
            Err(e) if e.is_timeout() => Err(LlmError::Timeout),
            Err(e) => Err(LlmError::Network(e.to_string())),
        }
    }

    /// The client for text Claude writes: the key from `secrets` (one typed
    /// in the browser, else the configured one), the models and effort from
    /// the configuration. A malformed Anthropic setting makes Claude
    /// unavailable rather than stopping a server that writes with Gemini.
    pub fn from_config() -> Result<Self, LlmError> {
        let cfg = config();
        if let Some(problem) = &cfg.anthropic_problem {
            return Err(LlmError::ClaudeMisconfigured(problem.clone()));
        }
        let api_key = secrets::api_key(TextProvider::Anthropic).ok_or(LlmError::ClaudeNoApiKey)?;
        Self::new(
            api_key,
            cfg.anthropic_text_model.clone(),
            cfg.anthropic_summary_model.clone(),
            cfg.anthropic_effort.clone(),
        )
    }

    pub(crate) fn new(
        api_key: String,
        text_model: String,
        summary_model: String,
        effort: String,
    ) -> Result<Self, LlmError> {
        Ok(Self {
            http: http_client()?,
            key_header: key_header(&api_key)?,
            api_key,
            text_model,
            summary_model,
            effort,
            meter: Arc::new(Mutex::new(Usage::default())),
        })
    }

    pub fn text_model(&self) -> &str {
        &self.text_model
    }

    pub fn summary_model(&self) -> &str {
        &self.summary_model
    }

    /// Everything this client (and its clones) has been billed for so far.
    pub fn usage(&self) -> Usage {
        self.meter.lock().map(|usage| *usage).unwrap_or_default()
    }

    fn add_usage(&self, usage: &Usage) {
        if let Ok(mut meter) = self.meter.lock() {
            meter.add(usage);
        }
    }

    /// Plain text from the text model.
    pub async fn generate_text(&self, prompt: &str) -> Result<String, LlmError> {
        let response = self.billed(Call::Text, prompt).await?;
        output_text(&response).ok_or_else(|| LlmError::Malformed("no text in the response".into()))
    }

    /// A few words from the summary model (the download-name summary), at
    /// low effort and without a fallback.
    pub async fn generate_brief(&self, prompt: &str) -> Result<String, LlmError> {
        let response = self.billed(Call::Brief, prompt).await?;
        output_text(&response).ok_or_else(|| LlmError::Malformed("no text in the response".into()))
    }

    /// JSON from the text model, parsed into `T`. Only the prompt asks for
    /// JSON; fences and words around the object are tolerated.
    pub async fn generate_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T, LlmError> {
        let response = self
            .billed(Call::Text, &format!("{prompt}{JSON_SUFFIX}"))
            .await?;
        let text = output_text(&response)
            .ok_or_else(|| LlmError::Malformed("no text in the response".into()))?;
        let payload = json_payload(&text);
        serde_json::from_str(payload).map_err(|e| {
            LlmError::Malformed(format!(
                "{e}; payload starts with: {}",
                payload.chars().take(200).collect::<String>()
            ))
        })
    }

    fn model(&self, call: Call) -> &str {
        match call {
            Call::Text => &self.text_model,
            Call::Brief => &self.summary_model,
        }
    }

    fn effort(&self, call: Call) -> &str {
        match call {
            Call::Text => &self.effort,
            Call::Brief => BRIEF_EFFORT,
        }
    }

    /// Sends `prompt`, meters the response, then checks why it stopped.
    async fn billed(&self, call: Call, prompt: &str) -> Result<Value, LlmError> {
        let model = self.model(call);
        let body = messages_body(model, prompt, self.effort(call), call);
        let started = Instant::now();
        let answered = self.post_messages(model, &body, call).await?;
        let usage = priced_usage(model, &answered.body, now_secs());
        let field = |name: &str| {
            answered
                .body
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or("")
        };
        tracing::info!(
            model,
            served_by = field("model"),
            request_id = answered.request_id.as_str(),
            stop_reason = field("stop_reason"),
            input = usage.input_tokens,
            cached = usage.cached_tokens,
            output = usage.output_tokens,
            thinking = usage.thinking_tokens,
            micro_usd = usage.micro_usd,
            unpriced = usage.unpriced,
            latency_ms = started.elapsed().as_millis() as u64,
            attempts = answered.attempts,
            "Claude request"
        );
        self.add_usage(&usage);
        check_stop(&answered.body)?;
        Ok(answered.body)
    }

    /// `POST /messages` within `call`'s budget. Retries what `classify`
    /// allows, and a connection that never opened once, after the wait
    /// `next_wait` gives: none for an error that came after `FAST_FAIL` or
    /// that would leave the next attempt short of its `min_attempt`. A
    /// refused key is marked in `secrets`.
    async fn post_messages(
        &self,
        model: &str,
        body: &Value,
        call: Call,
    ) -> Result<Answered, LlmError> {
        let started = Instant::now();
        let deadline = started + call.budget();
        let mut connect_retried = false;
        for attempt in 0..=MAX_RETRIES {
            let sent = Instant::now();
            let request = request(
                &self.http,
                Method::POST,
                "messages",
                &self.key_header,
                call.falls_back(),
            )
            .timeout(deadline.saturating_duration_since(sent))
            .json(body);
            let (reason, answered_after, asked_wait, why) = match request.send().await {
                Ok(response) if response.status().is_success() => {
                    let request_id = Headers::of(response.headers()).request_id;
                    secrets::mark_accepted(TextProvider::Anthropic, &self.api_key);
                    let body = read_billed(response, &request_id).await?;
                    return Ok(Answered {
                        body,
                        request_id,
                        attempts: attempt + 1,
                    });
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let headers = Headers::of(response.headers());
                    let text = response.text().await.unwrap_or_default();
                    match classify(status, &headers, &text, Some(model)) {
                        Failure::KeyRejected => {
                            return Err(LlmError::ClaudeActiveKeyRejected(secrets::mark_rejected(
                                TextProvider::Anthropic,
                                &self.api_key,
                            )));
                        }
                        Failure::Stop(error) => return Err(error),
                        Failure::Retry => {
                            let error_type = api_error(&text).map(|e| e.kind).unwrap_or_default();
                            (
                                LlmError::Busy,
                                Some(sent.elapsed()),
                                headers.retry_after,
                                format!("{status} {error_type}, request {}", headers.request_id),
                            )
                        }
                    }
                }
                // Never sent, so never billed: worth one more try.
                Err(e) if e.is_connect() && !connect_retried => {
                    connect_retried = true;
                    (
                        LlmError::Network(e.to_string()),
                        None,
                        None,
                        "the connection did not open".to_string(),
                    )
                }
                Err(e) if e.is_connect() => return Err(LlmError::Network(e.to_string())),
                Err(e) if e.is_timeout() => {
                    tracing::warn!(
                        model,
                        attempt,
                        "Claude request timed out after {} s; Anthropic may still bill it",
                        sent.elapsed().as_secs()
                    );
                    return Err(LlmError::Timeout);
                }
                Err(e) => return Err(LlmError::Network(e.to_string())),
            };
            let Some(wait) = next_wait(
                call,
                attempt,
                answered_after,
                started.elapsed(),
                asked_wait,
                random(),
            ) else {
                tracing::warn!(model, attempt, "Claude request not retried: {why}");
                return Err(reason);
            };
            tracing::warn!(
                model,
                attempt,
                wait_ms = wait.as_millis() as u64,
                "Claude request will be retried: {why}"
            );
            tokio::time::sleep(wait).await;
        }
        Err(LlmError::Busy)
    }
}

/// A success as Anthropic sent it.
struct Answered {
    body: Value,
    /// `request-id`, for the log.
    request_id: String,
    /// Attempts made, the successful one included.
    attempts: u32,
}

fn http_client() -> Result<reqwest::Client, LlmError> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        // The API never redirects, and a redirect to another host would
        // carry `x-api-key` along: reqwest strips only `authorization`.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| LlmError::Network(e.to_string()))
}

/// The key as a header value, marked sensitive. A key that cannot be a
/// header is not one Anthropic would accept.
fn key_header(api_key: &str) -> Result<HeaderValue, LlmError> {
    let mut value = HeaderValue::from_str(api_key).map_err(|_| LlmError::ClaudeKeyRejected)?;
    value.set_sensitive(true);
    Ok(value)
}

/// A request under `API_ROOT`: `path` is relative ("messages", "models").
/// The key and the API version are the only headers besides the content
/// type that `.json()` adds, and the fallback beta when `fallback` is set.
fn request(
    http: &reqwest::Client,
    method: Method,
    path: &str,
    key: &HeaderValue,
    fallback: bool,
) -> reqwest::RequestBuilder {
    let request = http
        .request(method, format!("{API_ROOT}/{path}"))
        .header("x-api-key", key.clone())
        .header("anthropic-version", API_VERSION);
    if fallback {
        request.header("anthropic-beta", FALLBACK_BETA)
    } else {
        request
    }
}

/// A Messages request: the prompt as the one user turn, after a fixed
/// `system` line, at `effort`. A text-model request may fall back.
fn messages_body(model: &str, prompt: &str, effort: &str, call: Call) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": call.max_tokens(),
        "system": SYSTEM,
        "messages": [{ "role": "user", "content": prompt }],
        "output_config": { "effort": effort },
    });
    if call.falls_back() {
        body["fallbacks"] = json!("default");
    }
    body
}

/// The body of a success. It is billed whatever happens next, so one that
/// cannot be read is logged as spend the meter does not see.
async fn read_billed(response: reqwest::Response, request_id: &str) -> Result<Value, LlmError> {
    let text = response.text().await.map_err(|e| {
        tracing::warn!(
            request_id,
            "a Claude answer could not be read; Anthropic bills it, the meter does not count it"
        );
        if e.is_timeout() {
            LlmError::Timeout
        } else {
            LlmError::Malformed(e.to_string())
        }
    })?;
    serde_json::from_str(&text).map_err(|e| {
        tracing::warn!(
            request_id,
            "a Claude answer was not JSON; Anthropic bills it, the meter does not count it"
        );
        LlmError::Malformed(e.to_string())
    })
}

/// The text of every `text` block, joined. Thinking and fallback blocks are
/// skipped, so the first block is never assumed to be the text.
fn output_text(response: &Value) -> Option<String> {
    let text: String = response
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// An answer that finished passes; one that was cut off, refused or stopped
/// for another reason becomes a readable error.
fn check_stop(response: &Value) -> Result<(), LlmError> {
    match response.get("stop_reason").and_then(Value::as_str) {
        None | Some("end_turn" | "stop_sequence") => Ok(()),
        Some("max_tokens") => Err(LlmError::Malformed(
            "the answer was cut off before it finished".into(),
        )),
        Some("model_context_window_exceeded") => Err(LlmError::Malformed(
            "the prompt was too long for the model".into(),
        )),
        Some("refusal") => Err(LlmError::ClaudeRefused(
            refusal_reason(
                response
                    .pointer("/stop_details/category")
                    .and_then(Value::as_str),
            )
            .into(),
        )),
        Some(other) => Err(LlmError::Malformed(format!(
            "the answer stopped early ({other})"
        ))),
    }
}

/// A refusal's category in the teacher's words. Only `stop_reason` says that
/// a reply is a refusal; the category may be missing.
fn refusal_reason(category: Option<&str>) -> &'static str {
    match category {
        Some("cyber") => "computer security",
        Some("bio") => "biological safety",
        Some("frontier_llm") => "AI development",
        Some("reasoning_extraction") => "its own reasoning",
        _ => "usage policy",
    }
}

/// The JSON object in a reply: inside fences, or from the first `{` to the
/// last `}` when words come before or after it.
fn json_payload(text: &str) -> &str {
    let payload = strip_fences(text);
    match (payload.find('{'), payload.rfind('}')) {
        (Some(start), Some(end)) if start < end => &payload[start..=end],
        _ => payload,
    }
}

/// Token counts of one attempt, in `Usage`'s terms: `input` includes the
/// cache reads and writes, `output` excludes thinking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Tokens {
    input: u64,
    cached: u64,
    /// Cache writes; none while no request sends `cache_control`.
    written: u64,
    output: u64,
    thinking: u64,
}

/// A `usage` object (or one of its `iterations`). Anthropic counts cache
/// reads and writes apart from `input_tokens`, and thinking inside
/// `output_tokens`; missing fields count as zero.
fn tokens_of(usage: &Value) -> Tokens {
    let count = |pointer: &str| usage.pointer(pointer).and_then(Value::as_u64).unwrap_or(0);
    let fresh = count("/input_tokens");
    let written = count("/cache_creation_input_tokens");
    let cached = count("/cache_read_input_tokens");
    let output = count("/output_tokens");
    let thinking = count("/output_tokens_details/thinking_tokens").min(output);
    Tokens {
        input: fresh + written + cached,
        cached,
        written,
        output: output - thinking,
        thinking,
    }
}

/// One billed response as `Usage`. A response that fell back lists every
/// attempt in `usage.iterations`, each priced at its own model's rates (one
/// that names no model, like the response itself); otherwise the model that
/// served it sets the price, else the requested one. An attempt on a model
/// without a price is counted and flagged.
///
/// A refusal before any output is billed for some categories only; it is
/// counted here all the same, so the ledger may show a little more than
/// Anthropic charged, never less. Cache writes are priced at the input
/// rate, which is below Anthropic's; no request asks for caching, so a
/// write is logged.
fn priced_usage(requested: &str, response: &Value, at_secs: i64) -> Usage {
    let usage = response.get("usage").unwrap_or(&Value::Null);
    let served = response.get("model").and_then(Value::as_str);
    let response_rates = || {
        served
            .and_then(|m| rates_for(m, at_secs))
            .or_else(|| rates_for(requested, at_secs))
    };
    let attempts: Vec<(Tokens, Option<Rates>)> = match usage
        .get("iterations")
        .and_then(Value::as_array)
        .filter(|iterations| !iterations.is_empty())
    {
        Some(iterations) => iterations
            .iter()
            .map(|iteration| {
                let rates = match iteration.get("model").and_then(Value::as_str) {
                    Some(model) => rates_for(model, at_secs),
                    None => response_rates(),
                };
                (tokens_of(iteration), rates)
            })
            .collect(),
        None => vec![(tokens_of(usage), response_rates())],
    };
    let mut total = Usage {
        requests: 1,
        ..Usage::default()
    };
    let mut written = 0;
    for (tokens, rates) in attempts {
        total.input_tokens += tokens.input;
        total.cached_tokens += tokens.cached;
        total.output_tokens += tokens.output;
        total.thinking_tokens += tokens.thinking;
        written += tokens.written;
        match rates {
            Some(rates) => {
                total.micro_usd += cost_micro_usd(
                    rates,
                    tokens.input,
                    tokens.cached,
                    tokens.output,
                    tokens.thinking,
                );
            }
            None => total.unpriced = 1,
        }
    }
    if written > 0 {
        tracing::warn!(
            written,
            "Claude reported prompt-cache writes, priced at the input rate; no request asks for caching"
        );
    }
    total
}

/// Anthropic's error envelope, `{"type": "error", "error": {"type",
/// "message", "details"?}, "request_id"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiError {
    /// "rate_limit_error", "overloaded_error"...
    kind: String,
    /// Cut to `MESSAGE_CHARS`.
    message: String,
    /// `error.details.error_code`, such as `SPEND_CAP`.
    code: Option<String>,
}

/// The envelope of an error body, if that is what it is and it says
/// something.
fn api_error(body: &str) -> Option<ApiError> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    if parsed.get("type").and_then(Value::as_str) != Some("error") {
        return None;
    }
    let error = parsed.get("error")?;
    let message = error.get("message").and_then(Value::as_str)?.trim();
    if message.is_empty() {
        return None;
    }
    Some(ApiError {
        kind: error
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        message: message.chars().take(MESSAGE_CHARS).collect(),
        code: error
            .pointer("/details/error_code")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// What the headers of an error answer say. Read before its body.
#[derive(Debug, Clone, Default, PartialEq)]
struct Headers {
    /// `retry-after`, in seconds.
    retry_after: Option<Duration>,
    /// `x-should-retry: false`: Anthropic says the request must not be repeated.
    no_retry: bool,
    /// `request-id`, for the log.
    request_id: String,
}

impl Headers {
    fn of(headers: &HeaderMap) -> Self {
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
        };
        Self {
            retry_after: retry_after(text("retry-after")),
            no_retry: text("x-should-retry").is_some_and(|v| v.eq_ignore_ascii_case("false")),
            request_id: text("request-id").unwrap_or("").to_string(),
        }
    }
}

/// A `retry-after` value: seconds, possibly with a fraction.
fn retry_after(value: Option<&str>) -> Option<Duration> {
    let seconds: f64 = value?.trim().parse().ok()?;
    Duration::try_from_secs_f64(seconds).ok()
}

/// What to do about an answer that is not a success.
#[derive(Debug)]
enum Failure {
    /// 401: Anthropic does not accept the key.
    KeyRejected,
    /// 429, 500, 502, 503 or 529: may be tried again; `Busy` when it is not.
    Retry,
    /// Final.
    Stop(LlmError),
}

/// An error answer, from its status, headers and body. `model` is the model
/// a Messages request named (`None` for the key check). Only Anthropic's own
/// error envelope gives the words the teacher sees; any other body (a
/// proxy's page) is logged and replaced by a fixed sentence.
fn classify(status: u16, headers: &Headers, body: &str, model: Option<&str>) -> Failure {
    let error = api_error(body);
    if error.is_none() {
        tracing::warn!(
            status,
            request_id = headers.request_id.as_str(),
            body = %body.chars().take(MESSAGE_CHARS).collect::<String>(),
            "Anthropic answered without its error envelope"
        );
    }
    let said = |fixed: &str| {
        error
            .as_ref()
            .map_or_else(|| fixed.to_string(), |e| e.message.clone())
    };
    let about_limits = error.as_ref().is_some_and(|e| account_blocked(&e.message));
    let spend_cap = error
        .as_ref()
        .is_some_and(|e| e.code.as_deref() == Some(SPEND_CAP));
    match status {
        401 => Failure::KeyRejected,
        402 => Failure::Stop(LlmError::ClaudeAccountBlocked(said(NO_PAYMENT))),
        400 if about_limits => Failure::Stop(LlmError::ClaudeAccountBlocked(said(NO_REASON))),
        429 if spend_cap => Failure::Stop(LlmError::ClaudeAccountBlocked(said(NO_REASON))),
        404 => Failure::Stop(LlmError::ClaudeRejected(match model {
            Some(model) => format!(
                "the model {model} is not available to this API key; check ANTHROPIC_TEXT_MODEL and ANTHROPIC_SUMMARY_MODEL"
            ),
            None => said(NO_REASON),
        })),
        413 => Failure::Stop(LlmError::ClaudeRejected(TOO_LARGE.into())),
        429 | 500 | 502 | 503 | 529 if headers.no_retry => Failure::Stop(LlmError::Busy),
        429 | 500 | 502 | 503 | 529 => Failure::Retry,
        // Documented as retryable. Not repeated here on purpose: it comes
        // near the tunnel's limit, after work that may be billed.
        504 => Failure::Stop(LlmError::Timeout),
        500..=599 => Failure::Stop(LlmError::Busy),
        _ => Failure::Stop(LlmError::ClaudeRejected(said(NO_REASON))),
    }
}

/// Whether a 400's message says the account may not run requests: no
/// balance left, or a usage or spend limit reached. The key itself works.
fn account_blocked(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    ["credit balance", "usage limits", "spend limit"]
        .iter()
        .any(|words| message.contains(words))
}

/// The key check's answer: the generation path's reading of it, without
/// retries. Only a 401 means the key is wrong.
fn key_check_outcome(status: u16, headers: &Headers, body: &str) -> Result<(), LlmError> {
    match status {
        200..=299 => Ok(()),
        500..=599 => Err(LlmError::Busy),
        _ => Err(match classify(status, headers, body, None) {
            Failure::KeyRejected => LlmError::ClaudeKeyRejected,
            Failure::Retry => LlmError::Busy,
            Failure::Stop(error) => error,
        }),
    }
}

/// The wait before the retry after attempt `attempt` (0 for the first), not
/// yet jittered: the `retry-after` Anthropic asked for, else 1 s, then 2 s.
/// `None` once the retries are used up or the asked wait is too long.
fn retry_wait(attempt: u32, retry_after: Option<Duration>) -> Option<Duration> {
    if attempt >= MAX_RETRIES {
        return None;
    }
    match retry_after {
        Some(wait) => (wait <= MAX_RETRY_AFTER).then_some(wait),
        None => Some(Duration::from_millis(FIRST_BACKOFF_MS << attempt)),
    }
}

/// The wait before the attempt after `attempt`, or `None` when there is to
/// be none: the error answer took longer than `FAST_FAIL` (`answered_after`,
/// `None` for a connection that never opened), so the model may have been
/// at work; the retries are used up or Anthropic asked for too long a wait
/// (`retry_wait`); or the next attempt would not have its `min_attempt` of
/// `call`'s budget, of which `elapsed` is spent. The wait Anthropic asked
/// for is kept as it is; only the backoff is jittered, by `random`.
fn next_wait(
    call: Call,
    attempt: u32,
    answered_after: Option<Duration>,
    elapsed: Duration,
    asked: Option<Duration>,
    random: u64,
) -> Option<Duration> {
    if answered_after.is_some_and(|took| took > FAST_FAIL) {
        return None;
    }
    let wait = retry_wait(attempt, asked)?;
    let wait = if asked.is_some() {
        wait
    } else {
        jittered(wait, random)
    };
    may_retry(elapsed, wait, call.budget(), call.min_attempt()).then_some(wait)
}

/// Whether a retry after `wait` still leaves its attempt `min_attempt` of a
/// `budget` of which `elapsed` is spent.
fn may_retry(elapsed: Duration, wait: Duration, budget: Duration, min_attempt: Duration) -> bool {
    elapsed + wait + min_attempt <= budget
}

/// `wait` times a factor from 0.5 to 1.5 drawn from `random`, so requests
/// refused together do not all come back at once.
fn jittered(wait: Duration, random: u64) -> Duration {
    wait.mul_f64(0.5 + random as f64 / u64::MAX as f64)
}

/// A random number, from the standard library's randomly keyed hasher.
fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const KEY: &str = "sk-ant-api03-secret-test-key";

    /// Anthropic's error envelope, as documented.
    fn envelope(kind: &str, message: &str) -> String {
        json!({
            "type": "error",
            "error": { "type": kind, "message": message },
            "request_id": "req_011CSHoEeqs5C35K2UUqR7Fy",
        })
        .to_string()
    }

    const SPEND_CAP_BODY: &str = r#"{"type":"error","error":{"type":"rate_limit_error","message":"This organization has reached its monthly spend limit.","details":{"error_code":"enforced_spend_limit_reached"}},"request_id":"req_011CSHoEeqs5C35K2UUqR7Fy"}"#;
    const CLOUDFLARE_403: &str = "<html>\r\n<head><title>403 Forbidden</title></head>\r\n<body>\r\n<center><h1>403 Forbidden</h1></center>\r\n<hr><center>cloudflare</center>\r\n</body>\r\n</html>\r\n";
    const CREDIT: &str = "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.";
    const USAGE_LIMITS: &str = "You have reached your specified API usage limits. You will regain access on 2026-11-01 at 00:00 UTC.";

    fn names(request: &reqwest::Request) -> BTreeSet<&str> {
        request.headers().keys().map(|name| name.as_str()).collect()
    }

    /// Runs `test` with ANTHROPIC_BASE_URL and ANTHROPIC_AUTH_TOKEN pointed
    /// elsewhere, then puts them back. Changing the environment while other
    /// tests run is safe on Windows only; elsewhere `test` sees it as it is.
    fn with_another_base_url(test: impl FnOnce()) {
        #[cfg(windows)]
        let saved = ["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN"].map(|name| {
            let old = std::env::var_os(name);
            // SAFETY: always safe on Windows, even with other threads running.
            unsafe { std::env::set_var(name, "https://base-url.invalid/not-anthropic") };
            (name, old)
        });
        test();
        #[cfg(windows)]
        saved.into_iter().for_each(|(name, old)| {
            // SAFETY: as above.
            unsafe {
                match old {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        });
    }

    #[test]
    fn requests_go_to_the_fixed_root_with_one_key_header() {
        // Claude Code sets ANTHROPIC_BASE_URL for the processes it starts,
        // this test among them, to Anthropic's own host. Pointed elsewhere it
        // changes nothing: the client never reads it.
        with_another_base_url(fixed_root_and_headers);
    }

    fn fixed_root_and_headers() {
        let http = reqwest::Client::new();
        let key = key_header(KEY).unwrap();
        let body = messages_body("claude-sonnet-5-5", "Hi", "low", Call::Text);
        let text = request(&http, Method::POST, "messages", &key, true)
            .json(&body)
            .build()
            .unwrap();
        assert_eq!(text.url().as_str(), "https://api.anthropic.com/v1/messages");
        assert_eq!(
            names(&text),
            BTreeSet::from([
                "anthropic-beta",
                "anthropic-version",
                "content-type",
                "x-api-key"
            ])
        );
        assert_eq!(
            text.headers()["anthropic-beta"],
            "server-side-fallback-2026-07-01"
        );
        assert_eq!(text.headers()["anthropic-version"], "2023-06-01");
        assert_eq!(text.headers()["x-api-key"], KEY);
        assert!(text.headers()["x-api-key"].is_sensitive());
        assert!(!format!("{text:?}").contains("secret-test-key"), "{text:?}");

        // The brief and the key check send no beta header.
        let brief = request(&http, Method::POST, "messages", &key, false)
            .json(&body)
            .build()
            .unwrap();
        assert_eq!(
            names(&brief),
            BTreeSet::from(["anthropic-version", "content-type", "x-api-key"])
        );
        let check = request(&http, Method::GET, "models", &key, false)
            .query(&[("limit", "1")])
            .build()
            .unwrap();
        assert_eq!(
            check.url().as_str(),
            "https://api.anthropic.com/v1/models?limit=1"
        );
        assert_eq!(
            names(&check),
            BTreeSet::from(["anthropic-version", "x-api-key"])
        );
        assert!(matches!(
            key_header("sk-ant\nbroken"),
            Err(LlmError::ClaudeKeyRejected)
        ));
    }

    #[test]
    fn bodies_send_effort_and_nothing_the_models_refuse() {
        let keys = |body: &Value| -> BTreeSet<String> {
            body.as_object().unwrap().keys().cloned().collect()
        };
        let set = |names: &[&str]| -> BTreeSet<String> {
            names.iter().map(|name| name.to_string()).collect()
        };
        let text = messages_body("claude-sonnet-5-5", "Write.", "medium", Call::Text);
        assert_eq!(
            keys(&text),
            set(&[
                "fallbacks",
                "max_tokens",
                "messages",
                "model",
                "output_config",
                "system"
            ])
        );
        assert_eq!(text["model"], "claude-sonnet-5-5");
        assert_eq!(text["max_tokens"], 8_192);
        assert_eq!(text["system"], SYSTEM);
        assert_eq!(text["output_config"], json!({ "effort": "medium" }));
        assert_eq!(text["fallbacks"], "default");
        // One user turn, and it is the last: no assistant prefill.
        assert_eq!(
            text["messages"],
            json!([{ "role": "user", "content": "Write." }])
        );

        let brief = messages_body("claude-haiku-5-5", "Sum up.", BRIEF_EFFORT, Call::Brief);
        assert_eq!(
            keys(&brief),
            set(&["max_tokens", "messages", "model", "output_config", "system"])
        );
        assert_eq!(brief["max_tokens"], 1_024);
        assert_eq!(brief["output_config"], json!({ "effort": "low" }));

        for body in [&text, &brief] {
            let sent = body.to_string();
            for banned in [
                "thinking",
                "temperature",
                "top_p",
                "top_k",
                "cache_control",
                "stream",
                "stop_sequences",
            ] {
                assert!(!sent.contains(&format!("\"{banned}\"")), "{banned}: {sent}");
            }
        }

        // The client picks the model and effort per request.
        let client = ClaudeClient::new(
            KEY.into(),
            "claude-sonnet-5-5".into(),
            "claude-haiku-5-5".into(),
            "high".into(),
        )
        .unwrap();
        let brief = messages_body(
            client.model(Call::Brief),
            "x",
            client.effort(Call::Brief),
            Call::Brief,
        );
        assert_eq!(
            (&brief["model"], &brief["output_config"]["effort"]),
            (&json!("claude-haiku-5-5"), &json!("low"))
        );
        assert_eq!(
            (client.model(Call::Text), client.effort(Call::Text)),
            ("claude-sonnet-5-5", "high")
        );
        assert_eq!(
            JSON_SUFFIX,
            "\n\nRespond with a single JSON object and nothing else."
        );
    }

    #[test]
    fn text_comes_from_text_blocks_only() {
        let response = json!({
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-5-5",
            "content": [
                { "type": "thinking", "thinking": "", "signature": "EqQB" },
                { "type": "text", "text": "Speaker A: Hi" },
                { "type": "text", "text": " there.\n" },
            ],
            "stop_reason": "end_turn",
        });
        assert_eq!(
            output_text(&response).as_deref(),
            Some("Speaker A: Hi there.")
        );
        let fell_back = json!({ "content": [
            { "type": "fallback", "from": { "model": "claude-sonnet-5-5" }, "to": { "model": "claude-sonnet-5" } },
            { "type": "text", "text": "Speaker A: Hello." },
        ]});
        assert_eq!(
            output_text(&fell_back).as_deref(),
            Some("Speaker A: Hello.")
        );
        // A refusal before any output has no content at all.
        assert_eq!(output_text(&json!({ "content": [] })), None);
        assert_eq!(
            output_text(&json!({ "content": [{ "type": "thinking", "thinking": "" }] })),
            None
        );
    }

    #[test]
    fn usage_counts_cache_and_thinking_once() {
        let response = json!({
            "model": "claude-sonnet-5-5",
            "usage": {
                "input_tokens": 1_000,
                "cache_creation_input_tokens": 200,
                "cache_read_input_tokens": 300,
                "cache_creation": { "ephemeral_5m_input_tokens": 200, "ephemeral_1h_input_tokens": 0 },
                "output_tokens": 900,
                "output_tokens_details": { "thinking_tokens": 400 },
            },
        });
        let usage = priced_usage("claude-sonnet-5-5", &response, 0);
        assert_eq!(
            (
                usage.requests,
                usage.input_tokens,
                usage.cached_tokens,
                usage.output_tokens,
                usage.thinking_tokens,
                usage.unpriced
            ),
            (1, 1_500, 300, 500, 400, 0)
        );
        // Cache writes would be priced at the plain input rate (no request
        // asks for caching), so the price is pinned without them: 1,000
        // fresh in at 2, 300 cached at 0.10, 500 out + 400 thinking at 10.
        let uncached = json!({
            "model": "claude-sonnet-5-5",
            "usage": {
                "input_tokens": 1_000,
                "cache_read_input_tokens": 300,
                "output_tokens": 900,
                "output_tokens_details": { "thinking_tokens": 400 },
            },
        });
        let usage = priced_usage("claude-sonnet-5-5", &uncached, 0);
        assert_eq!(usage.micro_usd, 2_000 + 30 + 9_000);
        // Without the breakdown it is all output, at the same price.
        let plain = json!({
            "model": "claude-sonnet-5-5",
            "usage": { "input_tokens": 1_000, "cache_read_input_tokens": 300, "output_tokens": 900 },
        });
        let usage = priced_usage("claude-sonnet-5-5", &plain, 0);
        assert_eq!(
            (usage.output_tokens, usage.thinking_tokens, usage.micro_usd),
            (900, 0, 11_030)
        );
        // Thinking never counts for more than the output it is part of.
        let odd = json!({ "usage": { "output_tokens": 100, "output_tokens_details": { "thinking_tokens": 400 } } });
        let usage = priced_usage("claude-sonnet-5-5", &odd, 0);
        assert_eq!((usage.output_tokens, usage.thinking_tokens), (0, 100));

        // The model that served sets the price, else the requested one.
        let tokens = json!({ "input_tokens": 1_000, "output_tokens": 100 });
        let served = json!({ "model": "claude-opus-5-5", "usage": tokens });
        assert_eq!(
            priced_usage("claude-sonnet-5-5", &served, 0).micro_usd,
            4_000 + 2_000
        );
        let dated = json!({ "model": "claude-sonnet-5-5-20261001", "usage": tokens });
        assert_eq!(
            priced_usage("claude-sonnet-5-5", &dated, 0).micro_usd,
            2_000 + 1_000
        );
        let unknown = priced_usage(
            "claude-x",
            &json!({ "model": "claude-x", "usage": tokens }),
            0,
        );
        assert_eq!((unknown.unpriced, unknown.micro_usd), (1, 0));
        assert_eq!(unknown.input_tokens, 1_000);

        // A fallback: the declined attempt at Sonnet 5.5's rates, the answer
        // at Sonnet 5's; top-level usage covers the answer only.
        let attempt = |kind: &str, model: &str, output: u64| {
            json!({
                "type": kind,
                "model": model,
                "input_tokens": 1_000,
                "output_tokens": output,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0,
            })
        };
        let fell_back = json!({
            "model": "claude-sonnet-5",
            "usage": {
                "input_tokens": 1_000,
                "output_tokens": 400,
                "iterations": [
                    attempt("message", "claude-sonnet-5-5", 0),
                    attempt("fallback_message", "claude-sonnet-5", 400),
                ],
            },
        });
        let usage = priced_usage("claude-sonnet-5-5", &fell_back, 0);
        assert_eq!(
            (
                usage.requests,
                usage.input_tokens,
                usage.output_tokens,
                usage.unpriced
            ),
            (1, 2_000, 400, 0)
        );
        assert_eq!(usage.micro_usd, 2_000 + 2_000 + 4_000);
        let unpriced = json!({
            "model": "claude-opus-4-8",
            "usage": { "iterations": [
                attempt("message", "claude-sonnet-5-5", 0),
                attempt("fallback_message", "claude-opus-4-8", 400),
            ]},
        });
        let usage = priced_usage("claude-sonnet-5-5", &unpriced, 0);
        assert_eq!(
            (usage.input_tokens, usage.micro_usd, usage.unpriced),
            (2_000, 2_000, 1)
        );
        // An attempt that names no model is priced as the response is.
        let unnamed = json!({
            "model": "claude-opus-5-5",
            "usage": { "iterations": [{ "type": "message", "input_tokens": 1_000, "output_tokens": 100 }] },
        });
        let usage = priced_usage("claude-sonnet-5-5", &unnamed, 0);
        assert_eq!((usage.micro_usd, usage.unpriced), (4_000 + 2_000, 0));
        let usage = priced_usage(
            "claude-sonnet-5-5",
            &json!({ "usage": unnamed["usage"].clone() }),
            0,
        );
        assert_eq!((usage.micro_usd, usage.unpriced), (2_000 + 1_000, 0));
    }

    #[test]
    fn stop_reasons_become_readable_errors() {
        let stop = |reason: Value, details: Value| {
            check_stop(&json!({ "stop_reason": reason, "stop_details": details }))
        };
        assert!(stop(json!("end_turn"), Value::Null).is_ok());
        assert!(stop(json!("stop_sequence"), Value::Null).is_ok());
        assert!(check_stop(&json!({ "content": [] })).is_ok());
        assert!(matches!(
            stop(json!("max_tokens"), Value::Null),
            Err(LlmError::Malformed(message)) if message.contains("cut off")
        ));
        assert!(matches!(
            stop(json!("model_context_window_exceeded"), Value::Null),
            Err(LlmError::Malformed(message)) if message.contains("too long")
        ));
        assert!(matches!(
            stop(json!("refusal"), Value::Null),
            Err(LlmError::ClaudeRefused(reason)) if reason == "usage policy"
        ));
        let category = |category: &str| json!({ "type": "refusal", "category": category, "explanation": null, "recommended_model": null });
        assert!(matches!(
            stop(json!("refusal"), category("cyber")),
            Err(LlmError::ClaudeRefused(reason)) if reason == "computer security"
        ));
        assert!(matches!(
            stop(json!("refusal"), category("general_harms")),
            Err(LlmError::ClaudeRefused(reason)) if reason == "usage policy"
        ));
        assert!(matches!(
            stop(json!("refusal"), json!({ "type": "refusal", "category": null })),
            Err(LlmError::ClaudeRefused(reason)) if reason == "usage policy"
        ));
        assert!(matches!(
            stop(json!("pause_turn"), Value::Null),
            Err(LlmError::Malformed(message)) if message.contains("pause_turn")
        ));
        assert_eq!(refusal_reason(Some("bio")), "biological safety");
        assert_eq!(refusal_reason(Some("frontier_llm")), "AI development");
        assert_eq!(
            refusal_reason(Some("reasoning_extraction")),
            "its own reasoning"
        );
    }

    #[test]
    fn errors_are_classified_from_documented_bodies() {
        let none = Headers::default();
        let model = Some("claude-sonnet-5-5");
        let stop = |status: u16, body: &str| match classify(status, &none, body, model) {
            Failure::Stop(error) => error,
            other => panic!("{status}: {other:?}"),
        };
        assert!(matches!(
            classify(
                401,
                &none,
                &envelope("authentication_error", "invalid x-api-key"),
                model
            ),
            Failure::KeyRejected
        ));
        assert!(matches!(
            stop(402, &envelope("billing_error", "Your payment method was declined.")),
            LlmError::ClaudeAccountBlocked(message) if message == "Your payment method was declined."
        ));
        assert!(matches!(
            stop(402, ""),
            LlmError::ClaudeAccountBlocked(message) if message == NO_PAYMENT
        ));
        assert!(matches!(
            stop(400, &envelope("invalid_request_error", USAGE_LIMITS)),
            LlmError::ClaudeAccountBlocked(message) if message == USAGE_LIMITS
        ));
        assert!(matches!(
            stop(400, &envelope("invalid_request_error", CREDIT)),
            LlmError::ClaudeAccountBlocked(message) if message == CREDIT
        ));
        let roles = "messages: roles must alternate between \"user\" and \"assistant\"";
        assert!(matches!(
            stop(400, &envelope("invalid_request_error", roles)),
            LlmError::ClaudeRejected(message) if message == roles
        ));
        assert!(matches!(
            stop(403, &envelope("permission_error", "Your organization is not allowed to use this model in your region.")),
            LlmError::ClaudeRejected(message) if message.contains("region")
        ));
        assert!(matches!(
            stop(403, CLOUDFLARE_403),
            LlmError::ClaudeRejected(message) if message == NO_REASON
        ));
        assert!(matches!(
            stop(404, &envelope("not_found_error", "model: claude-x")),
            LlmError::ClaudeRejected(message)
                if message.contains("claude-sonnet-5-5") && message.contains("ANTHROPIC_TEXT_MODEL")
        ));
        assert!(matches!(
            stop(413, "<html><body>413 Request Entity Too Large</body></html>"),
            LlmError::ClaudeRejected(message) if message == TOO_LARGE
        ));
        assert!(matches!(
            stop(429, SPEND_CAP_BODY),
            LlmError::ClaudeAccountBlocked(message) if message.contains("spend limit")
        ));
        let rate = envelope(
            "rate_limit_error",
            "Number of request tokens has exceeded your per-minute rate limit",
        );
        let waiting = Headers {
            retry_after: Some(Duration::from_secs(7)),
            ..Headers::default()
        };
        assert!(matches!(
            classify(429, &waiting, &rate, model),
            Failure::Retry
        ));
        let told_not_to = Headers {
            no_retry: true,
            ..Headers::default()
        };
        assert!(matches!(
            classify(429, &told_not_to, &rate, model),
            Failure::Stop(LlmError::Busy)
        ));
        for (status, body) in [
            (500, envelope("api_error", "Internal server error")),
            (502, "<html>502 Bad Gateway</html>".to_string()),
            (503, String::new()),
            (529, envelope("overloaded_error", "Overloaded")),
        ] {
            assert!(
                matches!(classify(status, &none, &body, model), Failure::Retry),
                "{status}"
            );
        }
        assert!(matches!(
            stop(504, &envelope("timeout_error", "Request timed out")),
            LlmError::Timeout
        ));
        assert!(matches!(stop(501, ""), LlmError::Busy));
        // Redirects are not followed (the key would go along): one is final.
        assert!(matches!(
            stop(307, ""),
            LlmError::ClaudeRejected(message) if message == NO_REASON
        ));
        assert!(matches!(
            stop(409, &envelope("conflict_error", "Conflict")),
            LlmError::ClaudeRejected(message) if message == "Conflict"
        ));
        // The envelope is required: other JSON is not Anthropic's word.
        assert!(matches!(
            stop(400, r#"{"error":{"message":"from a proxy"}}"#),
            LlmError::ClaudeRejected(message) if message == NO_REASON
        ));
        let long = "x".repeat(1_000);
        assert!(matches!(
            stop(400, &envelope("invalid_request_error", &long)),
            LlmError::ClaudeRejected(message) if message.chars().count() == MESSAGE_CHARS
        ));
    }

    #[test]
    fn claude_errors_are_teacher_readable() {
        let none = Headers::default();
        let answers = [
            (
                402,
                envelope("billing_error", "Your payment method was declined."),
            ),
            (402, String::new()),
            (400, envelope("invalid_request_error", CREDIT)),
            (400, envelope("invalid_request_error", USAGE_LIMITS)),
            (
                400,
                envelope("invalid_request_error", "max_tokens: 100000 > 64000"),
            ),
            (403, CLOUDFLARE_403.to_string()),
            (403, "error code: 1015".to_string()),
            (404, envelope("not_found_error", "model: claude-x")),
            (413, "<html>413 Request Entity Too Large</html>".to_string()),
            (429, SPEND_CAP_BODY.to_string()),
            (504, envelope("timeout_error", "Request timed out")),
            (418, String::new()),
        ];
        for (status, body) in &answers {
            for model in [Some("claude-sonnet-5-5"), None] {
                let message = match classify(*status, &none, body, model) {
                    Failure::Stop(error) => error.to_string(),
                    other => panic!("{status}: {other:?}"),
                };
                assert!(
                    message.ends_with('.') && !message.ends_with(".."),
                    "{message}"
                );
                for code in ["(4", "(5", "<", "402", "403", "404", "413", "418", "504"] {
                    assert!(!message.contains(code), "{code} in {message}");
                }
                assert!(!message.contains("1015"), "{message}");
            }
        }
        let key = key_check_outcome(
            401,
            &none,
            &envelope("authentication_error", "invalid x-api-key"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            key.contains("Claude Console") && key.ends_with('.'),
            "{key}"
        );
    }

    #[test]
    fn retry_after_is_read_and_capped() {
        let mut map = HeaderMap::new();
        map.insert("retry-after", HeaderValue::from_static("7"));
        map.insert("x-should-retry", HeaderValue::from_static("false"));
        map.insert(
            "request-id",
            HeaderValue::from_static("req_018EeWyXxfu5pfWkrYcMdjWG"),
        );
        assert_eq!(
            Headers::of(&map),
            Headers {
                retry_after: Some(Duration::from_secs(7)),
                no_retry: true,
                request_id: "req_018EeWyXxfu5pfWkrYcMdjWG".into(),
            }
        );
        assert_eq!(Headers::of(&HeaderMap::new()), Headers::default());
        map.insert("x-should-retry", HeaderValue::from_static("true"));
        assert!(!Headers::of(&map).no_retry);

        assert_eq!(retry_after(Some("7")), Some(Duration::from_secs(7)));
        assert_eq!(
            retry_after(Some(" 1.5 ")),
            Some(Duration::from_millis(1_500))
        );
        for unusable in [
            "soon",
            "-1",
            "NaN",
            "inf",
            "",
            "Wed, 21 Oct 2026 07:28:00 GMT",
        ] {
            assert_eq!(retry_after(Some(unusable)), None, "{unusable}");
        }
        assert_eq!(retry_after(None), None);

        // Asked waits up to 20 s are kept; a longer one stops the retries.
        assert_eq!(
            retry_wait(0, Some(Duration::from_secs(7))),
            Some(Duration::from_secs(7))
        );
        assert_eq!(
            retry_wait(1, Some(Duration::from_secs(20))),
            Some(Duration::from_secs(20))
        );
        assert_eq!(retry_wait(0, retry_after(Some("30"))), None);
        // Without one: 1 s, then 2 s, then no third retry.
        assert_eq!(retry_wait(0, None), Some(Duration::from_secs(1)));
        assert_eq!(retry_wait(1, None), Some(Duration::from_secs(2)));
        assert_eq!(retry_wait(2, None), None);
        assert_eq!(retry_wait(2, Some(Duration::from_secs(1))), None);
    }

    #[test]
    fn retries_stop_before_the_tunnel_limit() {
        let s = Duration::from_secs;
        assert!(may_retry(s(10), s(2), TEXT_BUDGET, TEXT_MIN_ATTEMPT));
        assert!(may_retry(s(38), s(2), TEXT_BUDGET, TEXT_MIN_ATTEMPT));
        assert!(!may_retry(s(39), s(2), TEXT_BUDGET, TEXT_MIN_ATTEMPT));
        // A 5xx that came late in the budget is not repeated.
        assert!(!may_retry(s(70), s(2), TEXT_BUDGET, TEXT_MIN_ATTEMPT));
        assert!(may_retry(s(2), s(1), BRIEF_BUDGET, BRIEF_MIN_ATTEMPT));
        assert!(!may_retry(s(12), s(4), BRIEF_BUDGET, BRIEF_MIN_ATTEMPT));
        // Two quick failures and both waits fit easily.
        assert!(may_retry(
            FAST_FAIL * 2 + s(3),
            s(3),
            TEXT_BUDGET,
            TEXT_MIN_ATTEMPT
        ));

        // The backoff is spread from half to one and a half times.
        assert_eq!(jittered(s(2), 0), s(1));
        assert_eq!(jittered(s(2), u64::MAX), s(3));
        let middle = jittered(s(2), u64::MAX / 2);
        assert!(middle > Duration::from_millis(1_999) && middle < Duration::from_millis(2_001));

        // What the request loop does with a retryable answer. One that came
        // within FAST_FAIL is tried again after the jittered backoff...
        let ms = Duration::from_millis;
        assert_eq!(
            next_wait(Call::Text, 0, Some(s(10)), s(10), None, 0),
            Some(ms(500))
        );
        assert_eq!(
            next_wait(Call::Text, 1, Some(s(1)), s(3), None, u64::MAX),
            Some(s(3))
        );
        // ...but a 5xx or 429 that came late may follow billed work.
        assert_eq!(
            next_wait(Call::Text, 0, Some(ms(10_001)), ms(10_001), None, 0),
            None
        );
        assert_eq!(next_wait(Call::Brief, 0, Some(s(11)), s(11), None, 0), None);
        // A connection that never opened has no answer time: its connect
        // timeout is as long as FAST_FAIL.
        assert_eq!(
            next_wait(Call::Text, 0, None, ms(10_050), None, u64::MAX),
            Some(ms(1_500))
        );
        // Anthropic's own wait is kept as asked, up to 20 s.
        assert_eq!(
            next_wait(Call::Text, 0, Some(s(1)), s(1), Some(s(7)), 0),
            Some(s(7))
        );
        assert_eq!(
            next_wait(Call::Text, 0, Some(s(1)), s(1), Some(s(21)), 0),
            None
        );
        // No third retry, and none that would start short of its minimum.
        assert_eq!(next_wait(Call::Text, 2, Some(s(1)), s(5), None, 0), None);
        assert_eq!(
            next_wait(Call::Text, 1, Some(s(2)), s(39), Some(s(2)), 0),
            None
        );
        assert_eq!(
            next_wait(Call::Brief, 0, Some(s(1)), s(12), Some(s(4)), 0),
            None
        );
    }

    #[test]
    fn json_payload_is_found_inside_fences_or_prose() {
        let object = r#"{"items": [{"number": 1}]}"#;
        assert_eq!(json_payload(object), object);
        assert_eq!(json_payload(&format!("```json\n{object}\n```")), object);
        assert_eq!(json_payload(&format!("  ```\n{object}\n```  ")), object);
        assert_eq!(
            json_payload(&format!(
                "Here is the JSON:\n\n{object}\n\nI hope this helps."
            )),
            object
        );
        assert_eq!(
            json_payload(&format!("Here it is:\n```json\n{object}\n```")),
            object
        );
        assert_eq!(
            json_payload(&format!("```json\n{object}\n```\nThat covers the task.")),
            object
        );
        assert_eq!(json_payload("  no object here  "), "no object here");
        assert_eq!(json_payload("} backwards {"), "} backwards {");
    }

    #[test]
    fn key_check_statuses() {
        let none = Headers::default();
        assert!(key_check_outcome(200, &none, "").is_ok());
        assert!(matches!(
            key_check_outcome(
                401,
                &none,
                &envelope("authentication_error", "invalid x-api-key")
            ),
            Err(LlmError::ClaudeKeyRejected)
        ));
        // A working key whose account may not run requests is not a bad key.
        for (status, body) in [
            (
                402,
                envelope("billing_error", "Your payment method was declined."),
            ),
            (400, envelope("invalid_request_error", CREDIT)),
            (400, envelope("invalid_request_error", USAGE_LIMITS)),
            (429, SPEND_CAP_BODY.to_string()),
        ] {
            assert!(
                matches!(
                    key_check_outcome(status, &none, &body),
                    Err(LlmError::ClaudeAccountBlocked(_))
                ),
                "{status}"
            );
        }
        // Nor is a key that may not do this: the message says what to do.
        assert!(matches!(
            key_check_outcome(403, &none, &envelope("permission_error", "Identity verification is required.")),
            Err(LlmError::ClaudeRejected(message)) if message == "Identity verification is required."
        ));
        assert!(matches!(
            key_check_outcome(404, &none, &envelope("not_found_error", "Not found")),
            Err(LlmError::ClaudeRejected(message)) if message == "Not found"
        ));
        assert!(matches!(
            key_check_outcome(
                400,
                &none,
                &envelope("invalid_request_error", "limit: must be at most 1000")
            ),
            Err(LlmError::ClaudeRejected(_))
        ));
        for status in [429, 500, 502, 503, 504, 529] {
            assert!(
                matches!(key_check_outcome(status, &none, ""), Err(LlmError::Busy)),
                "{status}"
            );
        }
    }

    /// Free: the input tokens `prompt` would cost (`POST /messages/count_tokens`,
    /// sent the model, `system` and the message only: the sending fields
    /// `max_tokens`, `fallbacks` and `output_config` change no input token).
    async fn count_tokens(client: &ClaudeClient, call: Call, prompt: &str) -> Result<u64, String> {
        let mut body = messages_body(client.model(call), prompt, client.effort(call), call);
        if let Some(fields) = body.as_object_mut() {
            fields.remove("max_tokens");
            fields.remove("fallbacks");
            fields.remove("output_config");
        }
        let response = request(
            &client.http,
            Method::POST,
            "messages/count_tokens",
            &client.key_header,
            false,
        )
        .timeout(KEY_CHECK_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
        let status = response.status();
        let text = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "{status}: {}",
                text.chars().take(MESSAGE_CHARS).collect::<String>()
            ));
        }
        serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|count| count.get("input_tokens").and_then(Value::as_u64))
            .ok_or_else(|| format!("no input_tokens in {text}"))
    }

    /// Calls the real API with the Anthropic key from the environment
    /// (process, then Windows) and the models and effort of
    /// `ANTHROPIC_TEXT_MODEL`, `ANTHROPIC_SUMMARY_MODEL` and
    /// `ANTHROPIC_EFFORT` (defaults otherwise). First the free steps: the key
    /// check with the real key and a made-up one, and the token counts of the
    /// prompts. Then a part 4 topic, its expressive IELTS script (the
    /// longest), the questions of its first task and a download-name
    /// summary, each parsed and validated as the app does. The client's own
    /// "Claude request" log line, printed here, gives each step's served
    /// model, stop_reason, request id and attempts. Costs about $0.03:
    /// ask before running it. Its name keeps it out of `live_probe`:
    /// `cargo test --features server --no-default-features claude_paid_probe -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn claude_paid_probe() {
        use crate::domain::{FormatId, Passage, validate_passage, validate_task};
        use crate::infrastructure::config::{
            DEFAULT_ANTHROPIC_EFFORT, DEFAULT_ANTHROPIC_SUMMARY_MODEL,
            DEFAULT_ANTHROPIC_TEXT_MODEL, windows_environment,
        };
        use crate::infrastructure::prompts;

        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| windows_environment().remove("ANTHROPIC_API_KEY"))
            .expect("ANTHROPIC_API_KEY in the environment");
        let setting = |name: &str, default: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        let client = ClaudeClient::new(
            key.clone(),
            setting("ANTHROPIC_TEXT_MODEL", DEFAULT_ANTHROPIC_TEXT_MODEL),
            setting("ANTHROPIC_SUMMARY_MODEL", DEFAULT_ANTHROPIC_SUMMARY_MODEL),
            setting("ANTHROPIC_EFFORT", DEFAULT_ANTHROPIC_EFFORT),
        )
        .unwrap();
        println!(
            "text model {}, summary model {}, effort {}",
            client.text_model, client.summary_model, client.effort
        );
        let report = |label: &str, before: Usage, started: Instant, budget: Duration| {
            let after = client.usage();
            println!(
                "{label}: {:.1} s of {} s, in {} (cached {}), out {}, thinking {}, ${:.4}{}",
                started.elapsed().as_secs_f32(),
                budget.as_secs(),
                after.input_tokens - before.input_tokens,
                after.cached_tokens - before.cached_tokens,
                after.output_tokens - before.output_tokens,
                after.thinking_tokens - before.thinking_tokens,
                (after.micro_usd - before.micro_usd) as f64 / 1_000_000.0,
                if after.unpriced > before.unpriced {
                    " +?"
                } else {
                    ""
                }
            );
        };

        // Step 0, free.
        ClaudeClient::check_key(&key)
            .await
            .expect("the key is accepted");
        let bogus = ClaudeClient::check_key("sk-ant-api03-made-up-key-for-the-probe").await;
        assert!(
            matches!(bogus, Err(LlmError::ClaudeKeyRejected)),
            "{bogus:?}"
        );
        println!("key check: the real key passes, a made-up one is rejected");
        let format = FormatId::IeltsListening.format();
        let part = format.part(4).expect("IELTS has a part 4");
        let speakers = part.default_speakers.clone();
        let sample_topic =
            "A university lecture on how cities can keep their streets cool in summer heatwaves.";
        for (label, call, prompt) in [
            (
                "topic",
                Call::Text,
                prompts::topic_prompt(&format, part, ""),
            ),
            (
                "script",
                Call::Text,
                prompts::passage_prompt(&format, part, sample_topic, &speakers, true),
            ),
            (
                "summary",
                Call::Brief,
                prompts::summary_prompt(&[sample_topic.to_string()]),
            ),
        ] {
            match count_tokens(&client, call, &prompt).await {
                Ok(tokens) => println!("count_tokens {label} prompt: {tokens} input tokens"),
                Err(e) => println!("count_tokens {label} prompt failed: {e}"),
            }
        }

        // The paid steps.
        let (before, started) = (client.usage(), Instant::now());
        let topic = client
            .generate_text(&prompts::topic_prompt(&format, part, ""))
            .await
            .unwrap();
        report(&format!("topic -> {topic:?}"), before, started, TEXT_BUDGET);

        let (before, started) = (client.usage(), Instant::now());
        let text = client
            .generate_text(&prompts::passage_prompt(
                &format, part, &topic, &speakers, true,
            ))
            .await
            .unwrap();
        report("script (part 4, expressive)", before, started, TEXT_BUDGET);
        let labels: Vec<String> = speakers.iter().map(|s| s.label.clone()).collect();
        let passage = Passage::parse(part.number, topic.as_str(), &text, &labels)
            .unwrap_or_else(|e| panic!("the script does not parse ({e}); it starts: {text:.300}"))
            .for_speakers(&speakers);
        let issues = validate_passage(&passage, part, &speakers);
        println!(
            "script: {} words, {} lines; issues: {issues:?}",
            passage.word_count(),
            passage.lines.len()
        );

        let spec = part.tasks[0].clone();
        let prompt = prompts::task_prompt(part, &spec, &passage, &speakers);
        match count_tokens(&client, Call::Text, &format!("{prompt}{JSON_SUFFIX}")).await {
            Ok(tokens) => println!("count_tokens questions prompt (free): {tokens} input tokens"),
            Err(e) => println!("count_tokens questions prompt failed: {e}"),
        }
        let (before, started) = (client.usage(), Instant::now());
        let draft = client
            .generate_json::<prompts::TaskDraftDto>(&prompt)
            .await
            .unwrap();
        report(
            &format!("questions ({:?})", spec.kind),
            before,
            started,
            TEXT_BUDGET,
        );
        let task = draft.into_task(spec);
        let issues = validate_task(&task, Some(&passage));
        println!("questions: {} items; issues: {issues:?}", task.items.len());

        let (before, started) = (client.usage(), Instant::now());
        let reply = client
            .generate_brief(&prompts::summary_prompt(&[topic.clone()]))
            .await
            .unwrap();
        report("summary", before, started, BRIEF_BUDGET);
        let line = prompts::summary_line(&reply);
        println!("summary: {reply:?} -> line {line:?}");
        assert!(!line.is_empty());

        let spent = client.usage();
        println!("total: {} requests, {}", spent.requests, spent.cost_text());
        assert_eq!(spent.unpriced, 0, "a model is missing from the price table");
    }
}
