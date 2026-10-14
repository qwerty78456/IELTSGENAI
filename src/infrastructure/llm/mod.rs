//! Language-model and text-to-speech transport. One client per provider,
//! each with its own HTTP path, retry policy and usage meter: Gemini for
//! text and speech, Claude for text. Text goes through `TextClient`.

mod claude;
mod gemini;
mod pricing;
mod text;

pub use claude::ClaudeClient;
pub use gemini::{
    CatalogVoice, GeminiClient, LlmError, SpeechRequest, SpeechTurn, VoiceAssignment, VoiceDesign,
    VoiceQuery,
};
pub use text::TextClient;
