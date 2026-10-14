//! The API keys requests use, one per text provider (Gemini, Anthropic).
//!
//! A key from the environment or `.env` (see `config::KeyOrigin`) is used
//! while its company accepts it. Without one, or once it has been rejected,
//! the teacher may type a key into a browser on the server's own computer
//! (`ingress::Origin::Local`); it is kept here, in this process's memory
//! only, replaces the configured key until a restart, and is gone after one
//! unless they chose to write it to `.env`. A key pasted at a console key
//! prompt at startup (`infrastructure::finish`) is not kept here: it is saved
//! in `.env` and becomes the configured key. Each provider has slots of its
//! own, so a Claude key never replaces or marks the Gemini key, nor the
//! reverse. Keys are never logged or sent back.

use std::sync::RwLock;

use crate::domain::TextProvider;

use super::config::{KeyOrigin, config};

/// One key per text provider, indexed by `TextProvider as usize`.
struct KeySlots([RwLock<Option<String>>; TextProvider::ALL.len()]);

impl KeySlots {
    const fn new() -> Self {
        Self([const { RwLock::new(None) }; TextProvider::ALL.len()])
    }

    fn slot(&self, provider: TextProvider) -> &RwLock<Option<String>> {
        &self.0[provider as usize]
    }

    fn get(&self, provider: TextProvider) -> Option<String> {
        self.slot(provider)
            .read()
            .ok()
            .and_then(|slot| slot.clone())
    }

    fn set(&self, provider: TextProvider, key: String) {
        if let Ok(mut slot) = self.slot(provider).write() {
            *slot = Some(key);
        }
    }

    fn holds(&self, provider: TextProvider, key: &str) -> bool {
        self.slot(provider)
            .read()
            .is_ok_and(|slot| slot.as_deref() == Some(key))
    }

    /// Empties `provider`'s slot if it holds `key`, taking the write lock
    /// only then.
    fn forget(&self, provider: TextProvider, key: &str) {
        if self.holds(provider, key)
            && let Ok(mut slot) = self.slot(provider).write()
            && slot.as_deref() == Some(key)
        {
            *slot = None;
        }
    }
}

static BROWSER_KEYS: KeySlots = KeySlots::new();
/// The last key each company refused, until it accepts that key again.
static REJECTED: KeySlots = KeySlots::new();

/// Where a refused `key` came from: the browser key, the configured key
/// (`configured`, found in `origin`) or neither.
fn origin_of(
    key: &str,
    browser: Option<&str>,
    configured: Option<&str>,
    origin: Option<KeyOrigin>,
) -> &'static str {
    if browser == Some(key) {
        "the key entered in this browser"
    } else if configured == Some(key) {
        origin.map_or("the server", KeyOrigin::describe)
    } else {
        "this request"
    }
}

/// The key for `provider` entered in the browser during this run, if any.
pub fn browser_key(provider: TextProvider) -> Option<String> {
    BROWSER_KEYS.get(provider)
}

/// Keeps a key for `provider` entered in the browser (already checked with
/// its company).
pub fn set_browser_key(provider: TextProvider, key: String) {
    BROWSER_KEYS.set(provider, key);
}

/// The key for `provider` entered in the browser, else the configured one. A
/// browser key is only accepted when there is no configured key or it was
/// rejected (see `application::settings::entry_refusal`), so it always wins.
pub fn api_key(provider: TextProvider) -> Option<String> {
    browser_key(provider).or_else(|| config().api_key(provider).map(str::to_string))
}

/// Notes that `provider`'s company refused `key` and says where that key
/// came from.
pub fn mark_rejected(provider: TextProvider, key: &str) -> &'static str {
    REJECTED.set(provider, key.to_string());
    origin_of(
        key,
        browser_key(provider).as_deref(),
        config().api_key(provider),
        config().key_origin_of(provider),
    )
}

/// Forgets a rejection of `key` once `provider`'s company accepts it again.
pub fn mark_accepted(provider: TextProvider, key: &str) {
    REJECTED.forget(provider, key);
}

/// Whether the key requests for `provider` use now was rejected.
pub fn active_key_rejected(provider: TextProvider) -> bool {
    api_key(provider).is_some_and(|key| REJECTED.holds(provider, &key))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEMINI: TextProvider = TextProvider::Gemini;
    const ANTHROPIC: TextProvider = TextProvider::Anthropic;

    #[test]
    fn each_provider_keeps_its_own_keys() {
        for (one, other) in [(GEMINI, ANTHROPIC), (ANTHROPIC, GEMINI)] {
            let browser = KeySlots::new();
            let rejected = KeySlots::new();
            browser.set(other, "other-browser-key".into());
            rejected.set(other, "other-rejected-key".into());
            // Setting, rejecting and accepting keys of one provider...
            browser.set(one, "browser-key".into());
            rejected.set(one, "browser-key".into());
            rejected.forget(one, "other-rejected-key");
            assert!(rejected.holds(one, "browser-key"));
            rejected.forget(one, "browser-key");
            assert_eq!(rejected.get(one), None);
            rejected.set(one, "configured-key".into());
            // ...never touches the other provider's slots.
            assert_eq!(browser.get(other).as_deref(), Some("other-browser-key"));
            assert_eq!(rejected.get(other).as_deref(), Some("other-rejected-key"));
            assert_eq!(browser.get(one).as_deref(), Some("browser-key"));
            assert_eq!(rejected.get(one).as_deref(), Some("configured-key"));
            // The same key in both slots is still two rejections.
            rejected.set(one, "shared-key".into());
            rejected.set(other, "shared-key".into());
            rejected.forget(one, "shared-key");
            assert!(!rejected.holds(one, "shared-key"));
            assert!(rejected.holds(other, "shared-key"));
        }
    }

    #[test]
    fn a_rejected_key_names_where_it_came_from() {
        let configured = Some("configured-key");
        let browser = Some("browser-key");
        assert_eq!(
            origin_of("browser-key", browser, configured, Some(KeyOrigin::DotEnv)),
            "the key entered in this browser"
        );
        assert_eq!(
            origin_of(
                "configured-key",
                browser,
                configured,
                Some(KeyOrigin::Windows)
            ),
            "the Windows environment"
        );
        assert_eq!(
            origin_of("configured-key", None, configured, None),
            "the server"
        );
        assert_eq!(
            origin_of("old-key", browser, configured, Some(KeyOrigin::Process)),
            "this request"
        );
    }
}
