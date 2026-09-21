use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::domain::{Task, TaskRequest, ValidationIssue};

/// A generated question block plus validator findings. Items with errors are
/// still returned so the teacher can fix them rather than regenerate blindly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDraft {
    pub task: Task,
    pub issues: Vec<ValidationIssue>,
}

/// Generates one task of a part from its passage.
#[server]
pub async fn generate_task(request: TaskRequest) -> Result<TaskDraft, ServerFnError> {
    use crate::application::user_error;
    use crate::domain::validate_task;
    use crate::infrastructure::{llm::GeminiClient, prompts, rate_limiter};

    let (part, spec) = request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Task).map_err(ServerFnError::new)?;
    let client = GeminiClient::from_config().map_err(user_error)?;
    let prompt = prompts::task_prompt(&part, &spec, &request.passage, &request.speakers);
    let draft: prompts::TaskDraftDto = client.generate_json(&prompt).await.map_err(user_error)?;
    let task = draft.into_task(spec);
    let issues = validate_task(&task, Some(&request.passage));
    Ok(TaskDraft { task, issues })
}
