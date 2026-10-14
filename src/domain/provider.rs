//! Text providers: whose model writes a draft's text (topics, scripts,
//! questions and download-name summaries). Speech is always Gemini.
//!
//! The environment variable names live here although they are configuration:
//! the server's configuration, the server functions' signatures and the UI
//! all need them, and the domain is the one layer all three may depend on.

use serde::{Deserialize, Serialize};

/// Who writes the text. A page may choose one per run; without a choice the
/// server's `TEXT_PROVIDER` does. Never switched on its own: a failed request
/// is not repeated with the other provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextProvider {
    /// Gemini, by Google.
    Gemini,
    /// Claude, by Anthropic.
    Anthropic,
}

impl TextProvider {
    /// Every provider, in UI order; `ALL[p as usize] == p`.
    pub const ALL: [TextProvider; 2] = [TextProvider::Gemini, TextProvider::Anthropic];

    /// Stable key: the `TEXT_PROVIDER` value and the serde name.
    pub fn key(self) -> &'static str {
        match self {
            TextProvider::Gemini => "gemini",
            TextProvider::Anthropic => "anthropic",
        }
    }

    /// The provider whose key this is, exactly (the caller trims and lowercases).
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|provider| provider.key() == key)
    }

    /// Name in the UI: "Gemini (Google)".
    pub fn label(self) -> &'static str {
        match self {
            TextProvider::Gemini => "Gemini (Google)",
            TextProvider::Anthropic => "Claude (Anthropic)",
        }
    }

    /// The company that checks the key and runs the model.
    pub fn company(self) -> &'static str {
        match self {
            TextProvider::Gemini => "Google",
            TextProvider::Anthropic => "Anthropic",
        }
    }

    /// The environment variable (and `.env` setting) that holds the API key.
    pub fn key_env(self) -> &'static str {
        match self {
            TextProvider::Gemini => "GEMINI_API_KEY",
            TextProvider::Anthropic => "ANTHROPIC_API_KEY",
        }
    }

    /// The key's name in messages: "Gemini API key".
    pub fn key_name(self) -> &'static str {
        match self {
            TextProvider::Gemini => "Gemini API key",
            TextProvider::Anthropic => "Anthropic API key",
        }
    }

    /// The provider a run uses: the page's choice, else the server's
    /// default. Never the other one.
    pub fn resolve(choice: Option<Self>, default: Self) -> Self {
        choice.unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_providers_round_trip_their_keys() {
        for (index, provider) in TextProvider::ALL.into_iter().enumerate() {
            assert_eq!(provider as usize, index);
            assert_eq!(TextProvider::from_key(provider.key()), Some(provider));
            assert_eq!(
                serde_json::to_string(&provider).unwrap(),
                format!("\"{}\"", provider.key())
            );
            assert_eq!(
                serde_json::from_str::<TextProvider>(&format!("\"{}\"", provider.key())).unwrap(),
                provider
            );
        }
        assert_eq!(TextProvider::from_key("claude"), None);
        assert_eq!(TextProvider::from_key("Gemini"), None);
        assert_eq!(TextProvider::Anthropic.key_env(), "ANTHROPIC_API_KEY");
        assert_eq!(TextProvider::Gemini.key_env(), "GEMINI_API_KEY");
        assert_eq!(TextProvider::Anthropic.label(), "Claude (Anthropic)");
        assert_eq!(TextProvider::Gemini.company(), "Google");
    }

    #[test]
    fn a_choice_is_never_swapped_for_the_other_provider() {
        for default in TextProvider::ALL {
            assert_eq!(TextProvider::resolve(None, default), default);
            for choice in TextProvider::ALL {
                assert_eq!(TextProvider::resolve(Some(choice), default), choice);
            }
        }
    }
}
