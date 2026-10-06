use dioxus::prelude::*;
use uuid::Uuid;

/// A few words that sum up a draft for its download file names, from its
/// theme and topics (`GEMINI_SUMMARY_MODEL`, Flash-Lite by default). Returns
/// the reply's first line; the browser keeps its first five words.
/// `exam` names the saved exam the spend is booked to (none from the part page).
#[server]
pub async fn summarize_topics(
    topics: Vec<String>,
    exam: Option<Uuid>,
) -> Result<String, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{config::config, llm::GeminiClient, prompts, rate_limiter};

    rate_limiter::check(rate_limiter::Bucket::Naming).map_err(ServerFnError::new)?;
    if topics.iter().all(|t| t.trim().is_empty()) {
        return Err(ServerFnError::new("There is no topic to sum up"));
    }
    let model = &config().summary_model;
    let client = GeminiClient::from_config().map_err(user_error)?;
    let reply = client
        .generate_brief(model, &prompts::summary_prompt(&topics))
        .await;
    usage::record(UsageStep::Naming, exam, model, &client).await;
    reply
        .map(|text| prompts::summary_line(&text))
        .map_err(user_error)
}
