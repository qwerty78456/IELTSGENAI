//! PCM handling and rendering of an `AudioProgram` into one recording.

mod program;
mod wav;

pub use program::{render_program, Announcer, AudioError, ProgramAssets};
pub use wav::{Pcm16, SAMPLE_RATE};
