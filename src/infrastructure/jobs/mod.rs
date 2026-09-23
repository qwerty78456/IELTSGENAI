//! Background jobs: long-running synthesis that must outlive an HTTP request.

mod serve;
mod store;
mod worker;

pub use serve::serve_audio;
pub use store::{JobKind, JobRecord, JobState, JobStore};
pub use worker::{ensure_cleanup_running, spawn_exam_audio, spawn_part_audio};
