//! Prompt builders. Pure functions from domain values to text; the only place
//! that knows how to talk to the model about listening exams.

mod items;
mod passage;
mod topic;

pub use items::{TaskDraftDto, task_prompt};
pub use passage::passage_prompt;
pub use topic::topic_prompt;
