//! Text-to-speech: voice selection and passage synthesis strategies.

mod synthesize;
pub mod voices;

pub use synthesize::{TtsError, synthesize_passage};
