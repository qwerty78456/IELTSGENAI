//! Text-to-speech: voice selection, chunking and reuse of synthesised speech.

mod cache;
mod synthesize;
pub mod voices;

pub use cache::purge_older_than as purge_speech_cache;
pub use synthesize::{TtsError, synthesize_passage};
