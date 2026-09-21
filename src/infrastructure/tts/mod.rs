//! Text-to-speech: voice selection and passage synthesis strategies.

mod synthesize;
mod voices;

pub use synthesize::{synthesize_passage, TtsError};
