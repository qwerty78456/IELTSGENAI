//! Background jobs: long-running synthesis that must outlive an HTTP request.

mod serve;
mod store;
mod worker;

pub use serve::{serve_audio, serve_voice_sample};
pub use store::{JobKind, JobRecord, JobState, JobStore, now_secs};
pub use worker::{ensure_cleanup_running, remove_output, spawn_exam_audio, spawn_part_audio};

/// What a recording interrupted by a server stop says, on both pages.
pub const INTERRUPTED_MESSAGE: &str =
    "The server stopped before this recording was finished. Make the recording again.";
