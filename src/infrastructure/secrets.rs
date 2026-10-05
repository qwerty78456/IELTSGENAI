//! The Gemini API key requests use.
//!
//! A key from the environment or `.env` (see `config::KeyOrigin`) is used
//! while Google accepts it. Without one, or once Google has rejected it, the
//! teacher may type a key into the browser; it is kept here, in this
//! process's memory only, replaces the configured key until a restart, and is
//! gone after one unless they chose to write it to `.env`. Keys are never
//! logged or sent back.

use std::sync::RwLock;

use super::config::config;

static BROWSER_KEY: RwLock<Option<String>> = RwLock::new(None);
/// The last key Google refused, until Google accepts it again.
static REJECTED: RwLock<Option<String>> = RwLock::new(None);

/// The key entered in the browser during this run, if any.
pub fn browser_key() -> Option<String> {
    BROWSER_KEY.read().ok().and_then(|slot| slot.clone())
}

/// Keeps a key entered in the browser (already checked with Google).
pub fn set_browser_key(key: String) {
    if let Ok(mut slot) = BROWSER_KEY.write() {
        *slot = Some(key);
    }
}

/// The key entered in the browser, else the configured one. A browser key is
/// only accepted when there is no configured key or Google rejected it (see
/// `application::settings::entry_refusal`), so it always wins.
pub fn api_key() -> Option<String> {
    browser_key().or_else(|| config().gemini_api_key.clone())
}

/// Notes that Google refused `key` and says where that key came from.
pub fn mark_rejected(key: &str) -> &'static str {
    if let Ok(mut slot) = REJECTED.write() {
        *slot = Some(key.to_string());
    }
    if browser_key().as_deref() == Some(key) {
        "the key entered in this browser"
    } else if config().gemini_api_key.as_deref() == Some(key) {
        config()
            .key_origin
            .map_or("the server", |origin| origin.describe())
    } else {
        "this request"
    }
}

/// Forgets a rejection of `key` once Google accepts it again.
pub fn mark_accepted(key: &str) {
    if REJECTED
        .read()
        .is_ok_and(|slot| slot.as_deref() == Some(key))
        && let Ok(mut slot) = REJECTED.write()
        && slot.as_deref() == Some(key)
    {
        *slot = None;
    }
}

/// Whether Google rejected the key requests use now.
pub fn active_key_rejected() -> bool {
    let Some(key) = api_key() else {
        return false;
    };
    REJECTED
        .read()
        .is_ok_and(|slot| slot.as_deref() == Some(key.as_str()))
}
