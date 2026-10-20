use dioxus::prelude::*;
use uuid::Uuid;

use crate::domain::TextProvider;

/// A few words that sum up a draft for its download file names, from its
/// theme and topics, by the text provider's summary model
/// (`GEMINI_SUMMARY_MODEL`, Flash-Lite by default, or
/// `ANTHROPIC_SUMMARY_MODEL`, Haiku by default). Returns the reply's first
/// line; the browser keeps its first five words.
/// `exam` names the saved exam the spend is booked to (none from the part page);
/// `provider` is the page's text provider (none: `TEXT_PROVIDER`).
#[server]
pub async fn summarize_topics(
    topics: Vec<String>,
    exam: Option<Uuid>,
    provider: Option<TextProvider>,
) -> Result<String, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{ingress, llm::TextClient, prompts, rate_limiter};

    ingress::require_app_access()?;
    if topics.iter().all(|t| t.trim().is_empty()) {
        return Err(ServerFnError::new("There is no topic to sum up"));
    }
    rate_limiter::check(rate_limiter::Bucket::Naming).map_err(ServerFnError::new)?;
    let client = TextClient::from_config(provider).map_err(user_error)?;
    let reply = client
        .generate_brief(&prompts::summary_prompt(&topics))
        .await;
    usage::record(
        UsageStep::Naming,
        exam,
        client.summary_model(),
        &client.usage(),
    )
    .await;
    reply
        .map(|text| prompts::summary_line(&text))
        .map_err(user_error)
}
