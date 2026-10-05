//! Language-model and text-to-speech transport. One client, one retry policy.

mod gemini;
mod pricing;

#[allow(unused_imports)] // the voice catalogue: `voice_live_probe` now, Voice Design next
pub use gemini::{CatalogVoice, VoiceQuery};
pub use gemini::{GeminiClient, LlmError, SpeechRequest, SpeechTurn, VoiceAssignment};
