//! Text-to-speech: voice selection and passage synthesis strategies.

mod synthesize;
mod voices;

pub use synthesize::{TtsError, synthesize_passage};
