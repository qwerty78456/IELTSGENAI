//! The Gemini API key requests use.
//!
//! A key from the environment or `.env` (see `config::KeyOrigin`) always wins.
//! Without one, the teacher may type a key into the browser; it is kept here,
//! in this process's memory only, and is gone after a restart unless they
//! chose to write it to `.env`. The key is never logged or sent back.

use std::sync::RwLock;

use super::config::config;

static BROWSER_KEY: RwLock<Option<String>> = RwLock::new(None);

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

/// The configured key, else the one entered in the browser.
pub fn api_key() -> Option<String> {
    config().gemini_api_key.clone().or_else(browser_key)
}
