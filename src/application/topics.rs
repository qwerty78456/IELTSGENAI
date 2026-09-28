use dioxus::prelude::*;
use uuid::Uuid;

use crate::domain::FormatId;

/// One scenario sentence for a part; `theme` narrows it and may be empty.
/// `exam` names the saved exam the spend is booked to (none from the part page).
#[server]
pub async fn suggest_topic(
    format: FormatId,
    part: u8,
    theme: String,
    exam: Option<Uuid>,
) -> Result<String, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{llm::GeminiClient, prompts, rate_limiter};

    rate_limiter::check(rate_limiter::Bucket::Topic).map_err(ServerFnError::new)?;
    let exam_format = format.format();
    let spec = exam_format
        .part(part)
        .ok_or_else(|| ServerFnError::new(format!("Part {part} does not exist")))?;
    let client = GeminiClient::from_config().map_err(user_error)?;
    let topic = client
        .generate_text(&prompts::topic_prompt(&exam_format, spec, &theme))
        .await;
    usage::record(UsageStep::Topic, exam, client.text_model(), &client).await;
    topic.map_err(user_error)
}
