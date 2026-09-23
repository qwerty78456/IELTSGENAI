//! The `Exam` aggregate: a format filled in part by part.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audio::AudioTrack;
use super::format::{ExamFormat, PartSpec, TaskSpec};
use super::passage::Passage;
use super::speaker::SpeakerConfig;
use super::task::{Answer, Task};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExamPart {
    pub spec: PartSpec,
    pub speakers: Vec<SpeakerConfig>,
    pub passage: Option<Passage>,
    pub tasks: Vec<Task>,
    pub audio: Option<AudioTrack>,
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
        }
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
