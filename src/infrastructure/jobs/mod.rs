//! Background jobs: long-running synthesis that must outlive an HTTP request.

mod store;
mod worker;

pub use store::{JobKind, JobState, JobStore};
pub use worker::{ensure_cleanup_running, spawn_exam_audio, spawn_part_audio};
