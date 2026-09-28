use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{Task, TaskRequest, ValidationIssue};

/// A generated question block plus validator findings. Items with errors are
/// still returned so the teacher can fix them rather than regenerate blindly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDraft {
    pub task: Task,
    pub issues: Vec<ValidationIssue>,
}

/// Generates one task of a part from its passage. `exam` names the saved
/// exam the spend is booked to (none from the part page).
#[server]
pub async fn generate_task(
    request: TaskRequest,
    exam: Option<Uuid>,
) -> Result<TaskDraft, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::{UsageStep, validate_task};
    use crate::infrastructure::{llm::GeminiClient, prompts, rate_limiter};

    let (part, spec) = request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Task).map_err(ServerFnError::new)?;
    let client = GeminiClient::from_config().map_err(user_error)?;
    let prompt = prompts::task_prompt(&part, &spec, &request.passage, &request.speakers);
    let draft = client.generate_json::<prompts::TaskDraftDto>(&prompt).await;
    usage::record(UsageStep::Questions, exam, client.text_model(), &client).await;
    let task = draft.map_err(user_error)?.into_task(spec);
    let issues = validate_task(&task, Some(&request.passage));
    Ok(TaskDraft { task, issues })
}
