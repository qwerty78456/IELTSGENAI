//! Text-to-speech: the voice catalogue, designed voices, chunking, reuse of
//! synthesised speech and voice samples.

mod cache;
mod designed;
mod samples;
mod synthesize;
pub mod voices;

pub use cache::{Reuse, purge_older_than as purge_speech_cache};
pub(crate) use designed::create_schema as create_designed_voices_schema;
pub use designed::{
    app_made_voice_ids, delete_designed_voice, design_voice, designed_voices, find_voice,
};
pub use samples::{SAMPLE_KEEP_HOURS, make_sample, stored_sample};
pub use synthesize::{TtsError, synthesize_passage};
