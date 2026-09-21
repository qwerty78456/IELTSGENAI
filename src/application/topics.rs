use dioxus::prelude::*;

use crate::domain::FormatId;

/// One scenario sentence for a part; `theme` narrows it and may be empty.
#[server]
pub async fn suggest_topic(format: FormatId, part: u8, theme: String) -> Result<String, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{llm::GeminiClient, prompts, rate_limiter};

    rate_limiter::check(rate_limiter::Bucket::Topic).map_err(ServerFnError::new)?;
    let exam = format.format();
    let spec = exam.part(part).ok_or_else(|| ServerFnError::new(format!("Part {part} does not exist")))?;
    let client = GeminiClient::from_config().map_err(user_error)?;
    client.generate_text(&prompts::topic_prompt(&exam, spec, &theme)).await.map_err(user_error)
}
