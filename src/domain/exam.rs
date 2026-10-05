//! The `Exam` aggregate: a format filled in part by part.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audio::AudioTrack;
use super::format::{ExamFormat, PartSpec, TaskSpec};
use super::passage::Passage;
use super::speaker::SpeakerConfig;
use super::task::{Answer, Task};
use super::voice::speaker_change;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExamPart {
    pub spec: PartSpec,
    pub speakers: Vec<SpeakerConfig>,
    pub passage: Option<Passage>,
    pub tasks: Vec<Task>,
    pub audio: Option<AudioTrack>,
    /// The line-up, voices included, this part was last recorded with (in
    /// the exam recording): what the server sent back when the recording
    /// started. Empty until then, and for exams saved before 0.8.
    #[serde(default)]
    pub recorded_for: Vec<SpeakerConfig>,
}

impl ExamPart {
    pub fn from_spec(spec: PartSpec) -> Self {
        let speakers = spec.default_speakers.clone();
        Self {
            spec,
            speakers,
            passage: None,
            tasks: Vec::new(),
            audio: None,
            recorded_for: Vec::new(),
        }
    }

    /// The speakers now sound different from the line-up this part was
    /// recorded with (`speaker_change(..).recording`). Derived, never stored:
    /// false when `recorded_for` is empty, so an exam from before 0.8, or
    /// voices assigned when the catalogue loads, mark nothing. Whether a
    /// recording exists is for the caller to say.
    pub fn recording_stale(&self) -> bool {
        speaker_change(&self.recorded_for, &self.speakers).recording
    }

    /// Task specs that have no generated task yet.
    pub fn missing_tasks(&self) -> Vec<&TaskSpec> {
        self.spec
            .tasks
            .iter()
            .filter(|spec| !self.tasks.iter().any(|t| &t.spec == *spec))
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        self.passage.is_some() && self.missing_tasks().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEntry {
    pub number: u8,
    pub answer: Answer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exam {
    pub id: Uuid,
    pub format: ExamFormat,
    pub title: String,
    /// The overarching theme the teacher asked for ("news listening").
    pub theme: String,
    pub parts: Vec<ExamPart>,
}

impl Exam {
    pub fn new(format: ExamFormat, title: impl Into<String>, theme: impl Into<String>) -> Self {
        let parts = format
            .parts
            .iter()
            .cloned()
            .map(ExamPart::from_spec)
            .collect();
        Self {
            id: Uuid::new_v4(),
            format,
            title: title.into(),
            theme: theme.into(),
            parts,
        }
    }

    pub fn part(&self, number: u8) -> Option<&ExamPart> {
        self.parts.iter().find(|p| p.spec.number == number)
    }

    pub fn part_mut(&mut self, number: u8) -> Option<&mut ExamPart> {
        self.parts.iter_mut().find(|p| p.spec.number == number)
    }

    /// Every item's key in paper order.
    pub fn answer_key(&self) -> Vec<KeyEntry> {
        let mut key: Vec<KeyEntry> = self
            .parts
            .iter()
            .flat_map(|part| part.tasks.iter())
            .flat_map(|task| {
                task.answers().map(|(number, answer)| KeyEntry {
                    number,
                    answer: answer.clone(),
                })
            })
            .collect();
        key.sort_by_key(|entry| entry.number);
        key
    }

    pub fn is_complete(&self) -> bool {
        self.parts.iter().all(ExamPart::is_complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::format::ExamFormat;
    use crate::domain::speaker::Gender;
    use crate::domain::voice::{Voice, VoiceChoice, VoiceSource};

    fn voice(id: &str, gender: Gender) -> Voice {
        Voice {
            id: id.into(),
            name: id.into(),
            gender,
            accent: crate::domain::speaker::Accent::British,
            source: VoiceSource::Library,
            description: String::new(),
        }
    }

    #[test]
    fn a_recording_goes_stale_when_its_speakers_sound_different() {
        let mut part = Exam::new(ExamFormat::ielts_listening(), "Mock", "").parts[0].clone();
        assert!(part.recorded_for.is_empty());
        // Nothing recorded with voices yet: never stale.
        part.speakers[0].gender = Gender::Male;
        assert!(!part.recording_stale());

        let gender = part.speakers[0].gender;
        part.speakers[0].voice = VoiceChoice::Assigned(voice("en-gb-a", gender));
        let gender = part.speakers[1].gender;
        part.speakers[1].voice = VoiceChoice::Assigned(voice("en-gb-b", gender));
        part.recorded_for = part.speakers.clone();
        assert!(!part.recording_stale());

        let gender = part.speakers[1].gender;
        part.speakers[1].voice = VoiceChoice::Chosen(voice("en-gb-c", gender));
        assert!(part.recording_stale());
        part.recorded_for = part.speakers.clone();
        assert!(!part.recording_stale());
        part.speakers[0].role = crate::domain::speaker::SpeakerRole::Expert;
        assert!(part.recording_stale());
    }

    #[test]
    fn parts_saved_before_recorded_for_still_load() {
        let part = Exam::new(ExamFormat::hsg_national(), "Mock", "").parts[0].clone();
        let mut json = serde_json::to_value(&part).unwrap();
        json.as_object_mut().unwrap().remove("recorded_for");
        let loaded: ExamPart = serde_json::from_value(json).unwrap();
        assert!(loaded.recorded_for.is_empty());
        assert_eq!(loaded, part);
    }
}
