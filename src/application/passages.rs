use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::domain::{Passage, PassageRequest, ValidationIssue};

/// A generated script plus what the validator thinks of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassageDraft {
    pub passage: Passage,
    pub issues: Vec<ValidationIssue>,
}

/// Generates the script of one part. Parsing failures are returned as errors;
/// softer problems come back as `issues` for the teacher to judge.
#[server]
pub async fn generate_passage(request: PassageRequest) -> Result<PassageDraft, ServerFnError> {
    use crate::application::user_error;
    use crate::domain::validate_passage;
    use crate::infrastructure::{llm::GeminiClient, prompts, rate_limiter};

    let spec = request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Passage).map_err(ServerFnError::new)?;
    let exam = request.format.format();
    let client = GeminiClient::from_config().map_err(user_error)?;
    let text = client
        .generate_text(&prompts::passage_prompt(&exam, &spec, &request.topic, &request.speakers))
        .await
        .map_err(user_error)?;
    let labels: Vec<String> = request.speakers.iter().map(|s| s.label.clone()).collect();
    let passage = Passage::parse(request.part, request.topic.trim(), &text, &labels).map_err(user_error)?;
    let issues = validate_passage(&passage, &spec, &request.speakers);
    Ok(PassageDraft { passage, issues })
}
