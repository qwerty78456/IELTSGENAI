use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{Passage, PassageRequest, ValidationIssue};

/// A generated script plus what the validator thinks of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassageDraft {
    pub passage: Passage,
    pub issues: Vec<ValidationIssue>,
}

/// Generates the script of one part. Parsing failures are returned as errors;
/// softer problems come back as `issues` for the teacher to judge. `exam`
/// names the saved exam the spend is booked to (none from the part page).
#[server]
pub async fn generate_passage(
    request: PassageRequest,
    exam: Option<Uuid>,
) -> Result<PassageDraft, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{llm::GeminiClient, prompts, rate_limiter};

    let spec = request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Passage).map_err(ServerFnError::new)?;
    let exam_format = request.format.format();
    let client = GeminiClient::from_config().map_err(user_error)?;
    let text = client
        .generate_text(&prompts::passage_prompt(
            &exam_format,
            &spec,
            &request.topic,
            &request.speakers,
        ))
        .await;
    usage::record(UsageStep::Script, exam, client.text_model(), &client).await;
    let text = text.map_err(user_error)?;
    draft_from(&request, &spec, &text).map_err(user_error)
}

/// The draft of a generated script: parsed, noted as written for the
/// request's speakers (so a later speaker edit can tell it no longer fits)
/// and validated against them.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn draft_from(
    request: &PassageRequest,
    spec: &crate::domain::PartSpec,
    text: &str,
) -> Result<PassageDraft, crate::domain::DomainError> {
    let labels: Vec<String> = request.speakers.iter().map(|s| s.label.clone()).collect();
    let passage = Passage::parse(request.part, request.topic.trim(), text, &labels)?
        .for_speakers(&request.speakers);
    let issues = crate::domain::validate_passage(&passage, spec, &request.speakers);
    Ok(PassageDraft { passage, issues })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FormatId, SpeakerRole};

    #[test]
    fn drafts_remember_the_speakers_they_were_written_for() {
        let request = PassageRequest {
            format: FormatId::IeltsListening,
            part: 1,
            topic: "  Booking a room at a sports centre ".into(),
            speakers: FormatId::IeltsListening.format().parts[0]
                .default_speakers
                .clone(),
        };
        let spec = request.validate().unwrap();
        let draft = draft_from(
            &request,
            &spec,
            "Speaker A: Good morning, how can I help?\nSpeaker B: I'd like to book a room.",
        )
        .unwrap();
        assert_eq!(draft.passage.topic, "Booking a room at a sports centre");
        assert_eq!(draft.passage.written_for, request.speakers);
        assert!(
            draft.issues.iter().all(|i| !i.message.contains("rewrite")),
            "{:?}",
            draft.issues
        );

        // Edited afterwards, the speakers no longer fit the draft.
        let mut edited = request.speakers.clone();
        edited[0].role = SpeakerRole::Guide;
        assert!(draft.passage.speakers_changed(&edited).script);

        assert!(draft_from(&request, &spec, "no labels here").is_err());
    }
}
