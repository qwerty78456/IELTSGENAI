//! The Gemini API key: where it came from, and entering one in the browser.
//!
//! The key itself never travels back to the browser. A key typed in the
//! browser is accepted only by a server bound to a loopback address and only
//! when no operator key (environment or `.env`) exists; see `entry_refusal`.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Where the key requests use comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeySource {
    /// `GEMINI_API_KEY` in the server process's environment.
    Environment,
    /// `GEMINI_API_KEY` stored in the Windows user or machine environment.
    WindowsEnvironment,
    /// `GEMINI_API_KEY` in `.env`.
    DotEnv,
    /// Typed into the browser since the server started.
    Browser,
    /// No key anywhere: generation fails until one is set.
    Missing,
}

impl KeySource {
    pub fn describe(self) -> &'static str {
        match self {
            KeySource::Environment => "the server's environment",
            KeySource::WindowsEnvironment => "the Windows environment",
            KeySource::DotEnv => ".env",
            KeySource::Browser => "this browser (kept in the server's memory)",
            KeySource::Missing => "nowhere",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyStatus {
    pub source: KeySource,
    /// Whether this server accepts a key typed in the browser right now.
    pub can_enter: bool,
}

/// Why a key typed in the browser is refused, or `None` when it is accepted.
/// `loopback` is whether the server listens on 127.0.0.1 / ::1 only.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn entry_refusal(source: KeySource, loopback: bool) -> Option<&'static str> {
    match source {
        KeySource::Environment | KeySource::WindowsEnvironment | KeySource::DotEnv => {
            Some("This server already has a key from its environment or .env; change it there.")
        }
        _ if !loopback => Some(
            "This server is reachable from other computers, so it does not accept a key from the browser. Set GEMINI_API_KEY on the server and restart.",
        ),
        KeySource::Browser | KeySource::Missing => None,
    }
}

#[cfg(feature = "server")]
fn current_status() -> KeyStatus {
    use crate::infrastructure::{
        config::{KeyOrigin, config},
        secrets,
    };
    let source = match config().key_origin {
        Some(KeyOrigin::Process) => KeySource::Environment,
        Some(KeyOrigin::Windows) => KeySource::WindowsEnvironment,
        Some(KeyOrigin::DotEnv) => KeySource::DotEnv,
        None if secrets::browser_key().is_some() => KeySource::Browser,
        None => KeySource::Missing,
    };
    let loopback = config().address.ip().is_loopback();
    KeyStatus {
        source,
        can_enter: entry_refusal(source, loopback).is_none(),
    }
}

/// Where the key comes from. Never returns the key or any part of it.
#[server]
pub async fn api_key_status() -> Result<KeyStatus, ServerFnError> {
    Ok(current_status())
}

/// Checks `key` with Google and keeps it in memory; with `remember`, also
/// writes it (unencrypted) to `.env` so it survives a restart.
#[server]
pub async fn set_api_key(key: String, remember: bool) -> Result<KeyStatus, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{config, llm::GeminiClient, rate_limiter, secrets};

    rate_limiter::check(rate_limiter::Bucket::KeyEntry).map_err(ServerFnError::new)?;
    let status = current_status();
    let loopback = config::config().address.ip().is_loopback();
    if let Some(reason) = entry_refusal(status.source, loopback) {
        return Err(ServerFnError::new(reason));
    }
    let key = key.trim().to_string();
    if let Some(problem) = config::browser_key_problem(&key) {
        return Err(ServerFnError::new(problem));
    }
    GeminiClient::check_key(&key).await.map_err(user_error)?;
    if remember {
        config::remember_in_dotenv(&config::config().dotenv_path, "GEMINI_API_KEY", &key)
            .map_err(ServerFnError::new)?;
    }
    secrets::set_browser_key(key);
    tracing::info!(
        remembered = remember,
        "Gemini API key entered in the browser"
    );
    Ok(current_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_keys_and_public_servers_refuse_browser_keys() {
        for source in [
            KeySource::Environment,
            KeySource::WindowsEnvironment,
            KeySource::DotEnv,
        ] {
            assert!(entry_refusal(source, true).is_some());
            assert!(entry_refusal(source, false).is_some());
        }
        for source in [KeySource::Missing, KeySource::Browser] {
            assert!(entry_refusal(source, true).is_none());
            assert!(entry_refusal(source, false).is_some());
        }
    }
}
