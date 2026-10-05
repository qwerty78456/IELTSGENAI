//! Commands: what the teacher asks for. Each validates itself against the format.

use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::format::{FormatId, PartSpec, TaskSpec};
use super::passage::Passage;
use super::speaker::SpeakerConfig;
use super::validation::{first_error, validate_speakers};
use super::voice::{Voice, voice_conflicts};

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
        if let Some(error) = first_error(&issues) {
            return Err(DomainError::InvalidRequest(error.message.clone()));
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
/// tones and pauses according to the format's `AudioProgram`. A part whose
/// request is `fresh` gets a new take; the others and the announcements
/// reuse what was read before.
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
            part.validate()
                .map_err(|e| DomainError::InvalidRequest(format!("{}: {e}", spec.title)))?;
        }
        Ok(())
    }
}

/// Synthesise the recording of one passage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioRequest {
    pub passage: Passage,
    pub speakers: Vec<SpeakerConfig>,
    /// A new take: read the passage again instead of reusing speech made
    /// before for the same words and voices, and keep the new take in their
    /// place. Pays for this part's speech again.
    #[serde(default)]
    pub fresh: bool,
}

impl AudioRequest {
    /// Every speaker must have a voice of its own that matches its gender.
    ///
    /// Callers assign voices first and validate after: the browser runs
    /// `assign_voices` over the catalogue before it sends the request, and the
    /// server assigns again (same rule) before it calls this. A speaker still
    /// on `Auto` here means no fitting voice was free for it.
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
        if let Some(speaker) = self.speakers.iter().find(|s| s.voice.is_auto()) {
            return Err(DomainError::InvalidRequest(format!(
                "{} has no voice yet",
                speaker.label
            )));
        }
        // The ids come from the browser and go into requests, cache keys and logs.
        if let Some(speaker) = self
            .speakers
            .iter()
            .find(|s| s.voice_id().is_some_and(|id| Voice::check_id(id).is_err()))
        {
            return Err(DomainError::InvalidRequest(format!(
                "{} has a voice id that is not valid; choose another voice",
                speaker.label
            )));
        }
        if let Some(conflict) = voice_conflicts(&self.speakers).into_iter().next() {
            return Err(DomainError::InvalidRequest(conflict));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::speaker::{Accent, Gender, SpeakerRole};
    use crate::domain::voice::{Voice, VoiceChoice, VoiceSource};

    fn voice(id: &str, gender: Gender) -> Voice {
        Voice {
            id: id.into(),
            name: id.into(),
            gender,
            accent: Accent::British,
            source: VoiceSource::Library,
            description: String::new(),
        }
    }

    fn request(speakers: Vec<SpeakerConfig>) -> AudioRequest {
        let labels: Vec<String> = speakers.iter().map(|s| s.label.clone()).collect();
        let script: Vec<String> = labels.iter().map(|l| format!("{l}: Hello.")).collect();
        AudioRequest {
            passage: Passage::parse(1, "topic", &script.join("\n"), &labels).unwrap(),
            speakers,
            fresh: false,
        }
    }

    fn speaker(label: &str, gender: Gender, voice: VoiceChoice) -> SpeakerConfig {
        SpeakerConfig {
            voice,
            ..SpeakerConfig::new(label, gender, Accent::British, SpeakerRole::Guest)
        }
    }

    #[test]
    fn recording_refuses_auto_shared_or_wrong_gender() {
        let a = voice("en-gb-a", Gender::Female);
        let b = voice("en-gb-b", Gender::Female);
        let fine = request(vec![
            speaker(
                "Speaker A",
                Gender::Female,
                VoiceChoice::Assigned(a.clone()),
            ),
            speaker("Speaker B", Gender::Female, VoiceChoice::Chosen(b)),
        ]);
        assert_eq!(fine.validate(), Ok(()));

        let auto = request(vec![
            speaker(
                "Speaker A",
                Gender::Female,
                VoiceChoice::Assigned(a.clone()),
            ),
            speaker("Speaker B", Gender::Female, VoiceChoice::Auto),
        ]);
        assert_eq!(
            auto.validate(),
            Err(DomainError::InvalidRequest(
                "Speaker B has no voice yet".into()
            ))
        );

        let shared = request(vec![
            speaker(
                "Speaker A",
                Gender::Female,
                VoiceChoice::Assigned(a.clone()),
            ),
            speaker("Speaker B", Gender::Female, VoiceChoice::Chosen(a.clone())),
        ]);
        let error = shared.validate().unwrap_err().to_string();
        assert!(error.contains("share the voice en-gb-a"), "{error}");

        let forged = request(vec![speaker(
            "Speaker A",
            Gender::Female,
            VoiceChoice::Chosen(voice("en-gb-a\nforged log line", Gender::Female)),
        )]);
        let error = forged.validate().unwrap_err().to_string();
        assert!(error.contains("voice id that is not valid"), "{error}");

        let wrong = request(vec![speaker(
            "Speaker A",
            Gender::Male,
            VoiceChoice::Assigned(a),
        )]);
        let error = wrong.validate().unwrap_err().to_string();
        assert!(error.contains("choose a male voice"), "{error}");

        let exam = ExamAudioRequest {
            format: FormatId::HsgNational,
            parts: vec![shared],
        };
        let error = exam.validate().unwrap_err().to_string();
        assert!(error.starts_with("Part 1: "), "{error}");
    }

    #[test]
    fn audio_requests_without_fresh_reuse_earlier_takes() {
        let speakers = vec![speaker(
            "Speaker A",
            Gender::Female,
            VoiceChoice::Assigned(voice("en-gb-a", Gender::Female)),
        )];
        let mut json = serde_json::to_value(request(speakers)).unwrap();
        json.as_object_mut().unwrap().remove("fresh");
        let loaded: AudioRequest = serde_json::from_value(json).unwrap();
        assert!(!loaded.fresh);

        // A new take of one part travels on that part's request only.
        let fresh = AudioRequest {
            fresh: true,
            ..loaded.clone()
        };
        let exam = ExamAudioRequest {
            format: FormatId::HsgNational,
            parts: vec![loaded, fresh],
        };
        let json = serde_json::to_string(&exam).unwrap();
        let back: ExamAudioRequest = serde_json::from_str(&json).unwrap();
        let flags: Vec<bool> = back.parts.iter().map(|p| p.fresh).collect();
        assert_eq!(flags, [false, true]);
    }

    #[test]
    fn script_requests_ignore_voice_warnings() {
        let shared = voice("en-gb-a", Gender::Female);
        let speakers = FormatId::IeltsListening.format().parts[0]
            .default_speakers
            .iter()
            .map(|s| s.clone().with_voice(shared.clone()))
            .collect();
        let request = PassageRequest {
            format: FormatId::IeltsListening,
            part: 1,
            topic: "Booking a room at a sports centre".into(),
            speakers,
        };
        assert!(request.validate().is_ok());
    }
}
