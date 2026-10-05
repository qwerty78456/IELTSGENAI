//! The Gemini API key: where it came from, and entering one in the browser.
//!
//! The key itself never travels back to the browser. A key typed in the
//! browser is accepted only by a server bound to a loopback address, and only
//! when no operator key (environment or `.env`) exists or Google rejected it;
//! it then replaces that key in memory until a restart. See `entry_refusal`.

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
    /// Where the key requests use now comes from.
    pub source: KeySource,
    /// Google refused that key on the last request that used it.
    pub rejected: bool,
    /// Whether this server accepts a key typed in the browser right now.
    pub can_enter: bool,
    /// Whether writing a typed key to `.env` would outlast a restart: not when
    /// the process or Windows environment holds a key, which `.env` cannot override.
    pub can_remember: bool,
}

/// Why a key typed in the browser is refused, or `None` when it is accepted.
/// `rejected` is whether Google refused the key in use; `loopback` is whether
/// the server listens on 127.0.0.1 / ::1 only.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn entry_refusal(source: KeySource, rejected: bool, loopback: bool) -> Option<&'static str> {
    match source {
        _ if !loopback => Some(
            "This server is reachable from other computers, so it does not accept a key from the browser. Set GEMINI_API_KEY on the server and restart.",
        ),
        KeySource::Environment | KeySource::WindowsEnvironment | KeySource::DotEnv if !rejected => {
            Some(
                "This server already has a working key from its environment or .env; change it there.",
            )
        }
        _ => None,
    }
}

/// Whether a key written to `.env` is the one used after a restart, given
/// where the configured key (if any) comes from.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn can_remember(configured: Option<KeySource>) -> bool {
    matches!(configured, None | Some(KeySource::DotEnv))
}

/// Whether this server listens on a loopback address only (127.0.0.1, ::1):
/// then only this computer reaches it. What changes the operator's Google
/// project (a key typed in the browser, designing or deleting voices) is
/// allowed only then.
#[cfg(feature = "server")]
pub(crate) fn local_server() -> bool {
    crate::infrastructure::config::config()
        .address
        .ip()
        .is_loopback()
}

#[cfg(feature = "server")]
fn configured_source() -> Option<KeySource> {
    use crate::infrastructure::config::{KeyOrigin, config};
    config().key_origin.map(|origin| match origin {
        KeyOrigin::Process => KeySource::Environment,
        KeyOrigin::Windows => KeySource::WindowsEnvironment,
        KeyOrigin::DotEnv => KeySource::DotEnv,
    })
}

#[cfg(feature = "server")]
fn current_status() -> KeyStatus {
    use crate::infrastructure::secrets;
    let configured = configured_source();
    let source = if secrets::browser_key().is_some() {
        KeySource::Browser
    } else {
        configured.unwrap_or(KeySource::Missing)
    };
    let rejected = secrets::active_key_rejected();
    let loopback = local_server();
    KeyStatus {
        source,
        rejected,
        can_enter: entry_refusal(source, rejected, loopback).is_none(),
        can_remember: can_remember(configured),
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
    if let Some(reason) = entry_refusal(status.source, status.rejected, local_server()) {
        return Err(ServerFnError::new(reason));
    }
    if remember && !status.can_remember {
        let place = configured_source().map_or("the environment", KeySource::describe);
        return Err(ServerFnError::new(format!(
            "GEMINI_API_KEY in {place} takes priority over .env, so remembering this key in .env would not help. Fix the key in {place} instead, or use this one until the app restarts."
        )));
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
    fn working_operator_keys_and_public_servers_refuse_browser_keys() {
        for source in [
            KeySource::Environment,
            KeySource::WindowsEnvironment,
            KeySource::DotEnv,
        ] {
            assert!(entry_refusal(source, false, true).is_some());
            // A key Google rejected may be replaced, but only on a local server.
            assert!(entry_refusal(source, true, true).is_none());
            assert!(entry_refusal(source, true, false).is_some());
            assert!(entry_refusal(source, false, false).is_some());
        }
        for source in [KeySource::Missing, KeySource::Browser] {
            for rejected in [false, true] {
                assert!(entry_refusal(source, rejected, true).is_none());
                assert!(entry_refusal(source, rejected, false).is_some());
            }
        }
    }

    #[test]
    fn only_dotenv_or_no_key_lets_a_remembered_key_survive_a_restart() {
        assert!(can_remember(None));
        assert!(can_remember(Some(KeySource::DotEnv)));
        assert!(!can_remember(Some(KeySource::Environment)));
        assert!(!can_remember(Some(KeySource::WindowsEnvironment)));
    }
}
