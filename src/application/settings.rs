//! The Gemini API key: where it came from, and entering one in the browser.
//!
//! The key itself never travels back to the browser. A key typed in the
//! browser is accepted only from a browser on the server's own computer (a
//! `Local` request, `infrastructure::ingress`), and only when no operator key
//! (environment or `.env`) exists or Google rejected it; it then replaces that
//! key in memory until a restart. See `entry_refusal`.

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
    /// Whether this server accepts a key typed in this browser right now.
    pub can_enter: bool,
    /// Whether writing a typed key to `.env` would outlast a restart: not when
    /// the process or Windows environment holds a key, which `.env` cannot override.
    pub can_remember: bool,
}

/// Why a key typed in the browser is refused, or `None` when it is accepted.
/// `rejected` is whether Google refused the key in use; `local` is whether
/// the request comes from a browser on the server's own computer.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn entry_refusal(source: KeySource, rejected: bool, local: bool) -> Option<&'static str> {
    match source {
        _ if !local => Some(
            "API keys can be entered only in a browser on the server's own computer. Set GEMINI_API_KEY on the server and restart.",
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

#[cfg(feature = "server")]
fn configured_source() -> Option<KeySource> {
    use crate::infrastructure::config::{KeyOrigin, config};
    config().key_origin.map(|origin| match origin {
        KeyOrigin::Process => KeySource::Environment,
        KeyOrigin::Windows => KeySource::WindowsEnvironment,
        KeyOrigin::DotEnv => KeySource::DotEnv,
    })
}

/// The key's status as the browser that sent the request (`local` or not)
/// sees it.
#[cfg(feature = "server")]
fn current_status(local: bool) -> KeyStatus {
    use crate::infrastructure::secrets;
    let configured = configured_source();
    let source = if secrets::browser_key().is_some() {
        KeySource::Browser
    } else {
        configured.unwrap_or(KeySource::Missing)
    };
    let rejected = secrets::active_key_rejected();
    KeyStatus {
        source,
        rejected,
        can_enter: entry_refusal(source, rejected, local).is_none(),
        can_remember: can_remember(configured),
    }
}

/// Where the key comes from. Never returns the key or any part of it.
#[server]
pub async fn api_key_status() -> Result<KeyStatus, ServerFnError> {
    use crate::infrastructure::ingress;

    Ok(current_status(ingress::current().is_local()))
}

/// Checks `key` with Google and keeps it in memory; with `remember`, also
/// writes it (unencrypted) to `.env` so it survives a restart.
#[server]
pub async fn set_api_key(key: String, remember: bool) -> Result<KeyStatus, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{config, ingress, llm::GeminiClient, rate_limiter, secrets};

    let local = ingress::current().is_local();
    let status = current_status(local);
    // Refused requests do not use up the rate limit of the server's own computer.
    if let Some(reason) = entry_refusal(status.source, status.rejected, local) {
        return Err(ServerFnError::new(reason));
    }
    rate_limiter::check(rate_limiter::Bucket::KeyEntry).map_err(ServerFnError::new)?;
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
    Ok(current_status(local))
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
            // A key Google rejected may be replaced, but only from the
            // server's own computer.
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
        assert_eq!(
            entry_refusal(KeySource::Missing, false, false),
            Some(
                "API keys can be entered only in a browser on the server's own computer. Set GEMINI_API_KEY on the server and restart."
            )
        );
    }

    #[test]
    fn only_dotenv_or_no_key_lets_a_remembered_key_survive_a_restart() {
        assert!(can_remember(None));
        assert!(can_remember(Some(KeySource::DotEnv)));
        assert!(!can_remember(Some(KeySource::Environment)));
        assert!(!can_remember(Some(KeySource::WindowsEnvironment)));
    }
}
