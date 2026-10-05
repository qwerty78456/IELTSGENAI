//! Text-to-speech: the voice catalogue, chunking, reuse of synthesised
//! speech and voice samples.

mod cache;
mod samples;
mod synthesize;
pub mod voices;

pub use cache::{Reuse, purge_older_than as purge_speech_cache};
pub use samples::{SAMPLE_KEEP_HOURS, make_sample, stored_sample};
pub use synthesize::{TtsError, synthesize_passage};
