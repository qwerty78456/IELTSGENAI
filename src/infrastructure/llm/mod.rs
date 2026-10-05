//! Language-model and text-to-speech transport. One client, one retry policy.

mod gemini;
mod pricing;

pub use gemini::{
    CatalogVoice, GeminiClient, LlmError, SpeechRequest, SpeechTurn, VoiceAssignment, VoiceDesign,
    VoiceQuery,
};
