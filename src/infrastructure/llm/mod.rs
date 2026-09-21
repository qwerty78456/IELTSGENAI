//! Language-model and text-to-speech transport. One client, one retry policy.

mod gemini;

pub use gemini::{GeminiClient, LlmError, VoiceAssignment};
