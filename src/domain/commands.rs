//! Commands: what the teacher asks for. Each validates itself against the format.

use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::format::{FormatId, PartSpec, TaskSpec};
use super::passage::Passage;
use super::speaker::SpeakerConfig;
use super::validation::{has_errors, validate_speakers};

pub const MIN_TOPIC_CHARS: usize = 10;
pub const MAX_TOPIC_CHARS: usize = 500;

pub fn validate_topic(topic: &str) -> Result<(), DomainError> {
    let trimmed = topic.trim();
    if trimmed.is_empty() {
        return Err(DomainError::InvalidRequest(
            "Please describe the topic or scenario".into(),
        ));
    }
    if trimmed.chars().count() < MIN_TOPIC_CHARS {
        return Err(DomainError::InvalidRequest(format!(
            "The topic is too short; write at least {MIN_TOPIC_CHARS} characters"
        )));
    }
    if trimmed.chars().count() > MAX_TOPIC_CHARS {
        return Err(DomainError::InvalidRequest(format!(
            "The topic is too long; keep it under {MAX_TOPIC_CHARS} characters"
        )));
    }
    if !trimmed.chars().any(char::is_alphabetic) {
        return Err(DomainError::InvalidRequest(
            "The topic must contain words, not only numbers or symbols".into(),
        ));
    }
    Ok(())
}

fn find_part(format: FormatId, part: u8) -> Result<PartSpec, DomainError> {
    format.format().part(part).cloned().ok_or_else(|| {
        DomainError::InvalidRequest(format!("Part {part} does not exist in this format"))
    })
}

/// Generate the script of one part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassageRequest {
    pub format: FormatId,
    pub part: u8,
    pub topic: String,
    pub speakers: Vec<SpeakerConfig>,
}

impl PassageRequest {
    pub fn validate(&self) -> Result<PartSpec, DomainError> {
        validate_topic(&self.topic)?;
        let spec = find_part(self.format, self.part)?;
        let issues = validate_speakers(&spec, &self.speakers);
        if has_errors(&issues) {
            return Err(DomainError::InvalidRequest(issues[0].message.clone()));
        }
        Ok(spec)
    }
}

/// Generate one task (question block) of a part from its finished passage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRequest {
    pub format: FormatId,
    pub part: u8,
    /// Index into `PartSpec::tasks`.
    pub task_index: usize,
    pub passage: Passage,
    pub speakers: Vec<SpeakerConfig>,
}

impl TaskRequest {
    pub fn validate(&self) -> Result<(PartSpec, TaskSpec), DomainError> {
        let spec = find_part(self.format, self.part)?;
        let task = spec.tasks.get(self.task_index).cloned().ok_or_else(|| {
            DomainError::InvalidRequest(format!(
                "Part {} has no task #{}",
                self.part,
                self.task_index + 1
            ))
        })?;
        if self.passage.lines.is_empty() {
            return Err(DomainError::InvalidRequest(
                "Generate the script before the questions".into(),
            ));
        }
        Ok((spec, task))
    }
}

/// Render the whole exam recording: every part's passage plus announcements,
/// tones and pauses according to the format's `AudioProgram`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExamAudioRequest {
    pub format: FormatId,
    pub parts: Vec<AudioRequest>,
}

impl ExamAudioRequest {
    pub fn validate(&self) -> Result<(), DomainError> {
        let exam = self.format.format();
        for spec in &exam.parts {
            let part = self
                .parts
                .iter()
                .find(|p| p.passage.part == spec.number)
                .ok_or_else(|| {
                    DomainError::InvalidRequest(format!("{} has no script yet", spec.title))
                })?;
            part.validate()?;
        }
        Ok(())
    }
}

/// Synthesise the recording of one passage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioRequest {
    pub passage: Passage,
    pub speakers: Vec<SpeakerConfig>,
}

impl AudioRequest {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.passage.lines.is_empty() {
            return Err(DomainError::InvalidRequest(
                "There is no script to read".into(),
            ));
        }
        let labels: Vec<&str> = self.speakers.iter().map(|s| s.label.as_str()).collect();
        for used in self.passage.speakers_used() {
            if !labels.contains(&used.as_str()) {
                return Err(DomainError::InvalidRequest(format!(
                    "No voice configured for \"{used}\""
                )));
            }
        }
        Ok(())
    }
}
