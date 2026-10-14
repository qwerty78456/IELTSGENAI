//! The API keys (Gemini, Anthropic): where each came from, and entering one
//! in the browser.
//!
//! A key itself never travels back to the browser. A key typed in the
//! browser is accepted only from a browser on the server's own computer (a
//! `Local` request, `infrastructure::ingress`), and only when no operator key
//! (environment or `.env`) for that provider exists or its company rejected
//! it; it then replaces that key in memory until a restart. See
//! `entry_refusal`. Each provider's key is separate: entering or rejecting
//! one never touches the other.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::domain::TextProvider;

/// Where the key requests use comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeySource {
    /// The key's variable (`GEMINI_API_KEY`, `ANTHROPIC_API_KEY`) in the
    /// server process's environment.
    Environment,
    /// The key's variable stored in the Windows user or machine environment.
    WindowsEnvironment,
    /// The key's variable in `.env`.
    DotEnv,
    /// Typed into the browser since the server started.
    Browser,
    /// No key anywhere: requests that need it fail until one is set.
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

/// One provider's key, as the browser that asked sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyStatus {
    /// Where the key requests use now comes from.
    pub source: KeySource,
    /// Its company refused that key on the last request that used it.
    pub rejected: bool,
    /// Whether this server accepts a key typed in this browser right now.
    pub can_enter: bool,
    /// Whether writing a typed key to `.env` would outlast a restart: not when
    /// the process or Windows environment holds a key, which `.env` cannot override.
    pub can_remember: bool,
}

/// Both providers' keys, and who writes the text of a run whose page chose
/// no one. Shared by the pages through `ui::components::key_setup::KeyStatusCtx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyStatuses {
    /// `TEXT_PROVIDER`, the server's default.
    pub text_default: TextProvider,
    pub gemini: KeyStatus,
    pub anthropic: KeyStatus,
}

impl KeyStatuses {
    pub fn of(&self, provider: TextProvider) -> KeyStatus {
        match provider {
            TextProvider::Gemini => self.gemini,
            TextProvider::Anthropic => self.anthropic,
        }
    }
}

/// Where a provider's key box goes on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBoxPlace {
    /// Above every page: the server needs the key whatever a page chooses.
    Top,
    /// Under the text-provider choice of the page that chose this provider.
    BesideChoice,
    /// Nowhere: the key works, or nothing asks for it.
    Hidden,
}

/// Where `provider`'s key box goes, given its `status`, the server's default
/// text provider and whether this page chose `provider` (`chosen_here`). A
/// box shows only while the key is missing or rejected. The Gemini key is
/// always needed (recordings), and so is the default provider's, and a
/// rejected key should be replaced wherever it is used: those go at the top.
/// The other provider's missing key matters only to a page that chose it.
pub fn key_box_place(
    provider: TextProvider,
    status: KeyStatus,
    text_default: TextProvider,
    chosen_here: bool,
) -> KeyBoxPlace {
    if status.source != KeySource::Missing && !status.rejected {
        KeyBoxPlace::Hidden
    } else if provider == TextProvider::Gemini || provider == text_default || status.rejected {
        KeyBoxPlace::Top
    } else if chosen_here {
        KeyBoxPlace::BesideChoice
    } else {
        KeyBoxPlace::Hidden
    }
}

/// Why a key for `provider` typed in the browser is refused, or `None` when
/// it is accepted. `rejected` is whether its company refused the key in use;
/// `local` is whether the request comes from a browser on the server's own
/// computer.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn entry_refusal(
    provider: TextProvider,
    source: KeySource,
    rejected: bool,
    local: bool,
) -> Option<&'static str> {
    match source {
        _ if !local => Some(match provider {
            TextProvider::Gemini => {
                "API keys can be entered only in a browser on the server's own computer. Set GEMINI_API_KEY on the server and restart."
            }
            TextProvider::Anthropic => {
                "API keys can be entered only in a browser on the server's own computer. Set ANTHROPIC_API_KEY on the server and restart."
            }
        }),
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

/// Where `provider`'s configured key (environment or `.env`) comes from.
#[cfg(feature = "server")]
fn configured_source(provider: TextProvider) -> Option<KeySource> {
    use crate::infrastructure::config::{KeyOrigin, config};
    config().key_origin_of(provider).map(|origin| match origin {
        KeyOrigin::Process => KeySource::Environment,
        KeyOrigin::Windows => KeySource::WindowsEnvironment,
        KeyOrigin::DotEnv => KeySource::DotEnv,
    })
}

/// `provider`'s key status as the browser that sent the request (`local` or
/// not) sees it.
#[cfg(feature = "server")]
fn current_status(provider: TextProvider, local: bool) -> KeyStatus {
    use crate::infrastructure::secrets;
    let configured = configured_source(provider);
    let source = if secrets::browser_key(provider).is_some() {
        KeySource::Browser
    } else {
        configured.unwrap_or(KeySource::Missing)
    };
    let rejected = secrets::active_key_rejected(provider);
    KeyStatus {
        source,
        rejected,
        can_enter: entry_refusal(provider, source, rejected, local).is_none(),
        can_remember: can_remember(configured),
    }
}

/// Both keys' statuses as the browser that sent the request sees them.
#[cfg(feature = "server")]
fn current_statuses(local: bool) -> KeyStatuses {
    KeyStatuses {
        text_default: crate::infrastructure::config::config().text_provider,
        gemini: current_status(TextProvider::Gemini, local),
        anthropic: current_status(TextProvider::Anthropic, local),
    }
}

/// Where each key comes from. Never returns a key or any part of one.
#[server]
pub async fn api_key_status() -> Result<KeyStatuses, ServerFnError> {
    use crate::infrastructure::ingress;

    Ok(current_statuses(ingress::current().is_local()))
}

/// Checks `key` with `provider`'s company and keeps it in memory; with
/// `remember`, also writes it (unencrypted) to `.env` so it survives a
/// restart.
#[server]
pub async fn set_api_key(
    provider: TextProvider,
    key: String,
    remember: bool,
) -> Result<KeyStatuses, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{
        config, ingress,
        llm::{ClaudeClient, GeminiClient, LlmError},
        rate_limiter, secrets,
    };

    let local = ingress::current().is_local();
    let status = current_status(provider, local);
    // Refused requests do not use up the rate limit of the server's own computer.
    if let Some(reason) = entry_refusal(provider, status.source, status.rejected, local) {
        return Err(ServerFnError::new(reason));
    }
    // A malformed Anthropic setting keeps Claude off until a restart
    // (`ClaudeClient::from_config`), so a key entered now could not be used.
    if provider == TextProvider::Anthropic
        && let Some(problem) = &config::config().anthropic_problem
    {
        return Err(user_error(LlmError::ClaudeMisconfigured(problem.clone())));
    }
    rate_limiter::check(rate_limiter::Bucket::KeyEntry).map_err(ServerFnError::new)?;
    if remember && !status.can_remember {
        let place = configured_source(provider).map_or("the environment", KeySource::describe);
        return Err(ServerFnError::new(format!(
            "{} in {place} takes priority over .env, so remembering this key in .env would not help. Fix the key in {place} instead, or use this one until the app restarts.",
            provider.key_env()
        )));
    }
    let key = key.trim().to_string();
    if let Some(problem) = config::browser_key_problem(provider, &key) {
        return Err(ServerFnError::new(problem));
    }
    match provider {
        TextProvider::Gemini => GeminiClient::check_key(&key).await,
        TextProvider::Anthropic => ClaudeClient::check_key(&key).await,
    }
    .map_err(user_error)?;
    if remember {
        config::remember_in_dotenv(&config::config().dotenv_path, provider.key_env(), &key)
            .map_err(ServerFnError::new)?;
    }
    secrets::set_browser_key(provider, key);
    tracing::info!(
        remembered = remember,
        "{} entered in the browser",
        provider.key_name()
    );
    Ok(current_statuses(local))
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
            assert!(entry_refusal(TextProvider::Gemini, source, false, true).is_some());
            // A key Google rejected may be replaced, but only from the
            // server's own computer.
            assert!(entry_refusal(TextProvider::Gemini, source, true, true).is_none());
            assert!(entry_refusal(TextProvider::Gemini, source, true, false).is_some());
            assert!(entry_refusal(TextProvider::Gemini, source, false, false).is_some());
        }
        for source in [KeySource::Missing, KeySource::Browser] {
            for rejected in [false, true] {
                assert!(entry_refusal(TextProvider::Gemini, source, rejected, true).is_none());
                assert!(entry_refusal(TextProvider::Gemini, source, rejected, false).is_some());
            }
        }
        assert_eq!(
            entry_refusal(TextProvider::Gemini, KeySource::Missing, false, false),
            Some(
                "API keys can be entered only in a browser on the server's own computer. Set GEMINI_API_KEY on the server and restart."
            )
        );
    }

    #[test]
    fn anthropic_entry_refusal_names_its_variable() {
        let anthropic = TextProvider::Anthropic;
        assert_eq!(
            entry_refusal(anthropic, KeySource::Missing, false, false),
            Some(
                "API keys can be entered only in a browser on the server's own computer. Set ANTHROPIC_API_KEY on the server and restart."
            )
        );
        for source in [
            KeySource::Environment,
            KeySource::WindowsEnvironment,
            KeySource::DotEnv,
            KeySource::Browser,
            KeySource::Missing,
        ] {
            for rejected in [false, true] {
                // The same rules as for the Gemini key, under its own name.
                for local in [false, true] {
                    assert_eq!(
                        entry_refusal(anthropic, source, rejected, local).is_some(),
                        entry_refusal(TextProvider::Gemini, source, rejected, local).is_some()
                    );
                }
                if let Some(reason) = entry_refusal(anthropic, source, rejected, false) {
                    assert!(!reason.contains("GEMINI"), "{reason}");
                }
            }
        }
        assert_eq!(
            entry_refusal(anthropic, KeySource::DotEnv, false, true),
            Some(
                "This server already has a working key from its environment or .env; change it there."
            )
        );
    }

    #[test]
    fn key_boxes_go_where_the_teacher_needs_them() {
        use KeyBoxPlace::{BesideChoice, Hidden, Top};
        use TextProvider::{Anthropic, Gemini};
        let status = |source, rejected| KeyStatus {
            source,
            rejected,
            can_enter: true,
            can_remember: true,
        };
        let missing = status(KeySource::Missing, false);
        let working = status(KeySource::DotEnv, false);
        let rejected = status(KeySource::DotEnv, true);
        let rejected_browser_key = status(KeySource::Browser, true);
        // (provider, status, text_default, chosen_here) -> place
        let table = [
            // The Gemini key is always needed: recordings use it.
            (Gemini, missing, Gemini, false, Top),
            (Gemini, missing, Anthropic, false, Top),
            (Gemini, missing, Gemini, true, Top),
            (Gemini, rejected, Anthropic, false, Top),
            (Gemini, working, Gemini, true, Hidden),
            (Gemini, working, Anthropic, false, Hidden),
            // Claude by default: its key is needed on every page.
            (Anthropic, missing, Anthropic, false, Top),
            (Anthropic, missing, Anthropic, true, Top),
            // Gemini by default: only a page that chose Claude asks for it.
            (Anthropic, missing, Gemini, false, Hidden),
            (Anthropic, missing, Gemini, true, BesideChoice),
            // A rejected key is replaced wherever it shows.
            (Anthropic, rejected, Gemini, false, Top),
            (Anthropic, rejected, Gemini, true, Top),
            (Anthropic, rejected_browser_key, Gemini, false, Top),
            (Anthropic, working, Gemini, true, Hidden),
            (Anthropic, working, Anthropic, false, Hidden),
            (
                Anthropic,
                status(KeySource::Browser, false),
                Gemini,
                true,
                Hidden,
            ),
        ];
        for (provider, status, text_default, chosen_here, place) in table {
            assert_eq!(
                key_box_place(provider, status, text_default, chosen_here),
                place,
                "{provider:?} {status:?} default {text_default:?} chosen {chosen_here}"
            );
        }
    }

    #[test]
    fn statuses_are_read_per_provider() {
        let status = |source| KeyStatus {
            source,
            rejected: false,
            can_enter: false,
            can_remember: false,
        };
        let statuses = KeyStatuses {
            text_default: TextProvider::Gemini,
            gemini: status(KeySource::DotEnv),
            anthropic: status(KeySource::Missing),
        };
        assert_eq!(statuses.of(TextProvider::Gemini).source, KeySource::DotEnv);
        assert_eq!(
            statuses.of(TextProvider::Anthropic).source,
            KeySource::Missing
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
