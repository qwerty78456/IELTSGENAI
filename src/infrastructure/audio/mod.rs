//! PCM handling and rendering of an `AudioProgram` into one recording.

mod program;
mod wav;

pub use program::{Announcer, AudioError, ProgramAssets, render_program};
pub use wav::{Pcm16, SAMPLE_RATE, duration_ms_for_len};
