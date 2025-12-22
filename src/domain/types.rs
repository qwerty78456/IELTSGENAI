//! Core types and value objects

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// One of the four distinct parts of an IELTS listening practice session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListeningSection {
    /// Transactional Conversation (2 speakers)
    Section1,
    /// Guided Monologue (1 speaker)
    Section2,
    /// Academic Discussion (2 speakers)
    Section3,
    /// Academic Lecture (1 speaker)
    Section4,
}

impl ListeningSection {
    /// Returns the required number of speakers for this section
    pub fn required_speaker_count(&self) -> usize {
        match self {
            ListeningSection::Section1 => 2,
            ListeningSection::Section2 => 1,
            ListeningSection::Section3 => 2,
            ListeningSection::Section4 => 1,
        }
    }

    /// Returns a description of this section
    pub fn description(&self) -> &'static str {
        match self {
            ListeningSection::Section1 => "Transactional Conversation (2 speakers, everyday context)",
            ListeningSection::Section2 => "Guided Monologue (1 speaker, general context)",
            ListeningSection::Section3 => "Academic Discussion (2 speakers, education context)",
            ListeningSection::Section4 => "Academic Lecture (1 speaker, university context)",
        }
    }
}

/// Role of a speaker in the listening content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeakerRole {
    Student,
    Professor,
    Clerk,
    Receptionist,
    Guide,
    Other(String),
}

/// Accent variant for text-to-speech
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accent {
    British,
    American,
    Australian,
    Canadian,
    NewZealand,
}

/// Gender for speaker voice
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gender {
    Male,
    Female,
}

/// Configuration for a single speaker voice
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakerConfig {
    /// Internal identifier (e.g., "Speaker A")
    pub name: String,
    /// Gender of the voice
    pub gender: Gender,
    /// Accent variant
    pub accent: Accent,
    /// Role in the conversation
    pub role: SpeakerRole,
}

/// A single line of dialogue or monologue
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLine {
    /// Reference to a SpeakerConfig name
    pub speaker_id: String,
    /// The spoken content
    pub text: String,
    /// Optional start time for alignment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<Duration>,
    /// Optional end time for alignment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<Duration>,
}

/// The generated text content before audio synthesis
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningScript {
    /// Which section this script represents
    pub section: ListeningSection,
    /// The topic of the content
    pub topic: String,
    /// Ordered sequence of lines
    pub lines: Vec<ScriptLine>,
    /// Estimated duration of the script
    pub estimated_duration: Duration,
}

impl ListeningScript {
    /// Validates that the script conforms to section rules
    pub fn validate(&self, speakers: &[SpeakerConfig]) -> Result<(), String> {
        // Must have at least one line
        if self.lines.is_empty() {
            return Err("Script must have at least one line".to_string());
        }

        // Check that all speaker IDs exist in the configuration
        let speaker_names: Vec<&str> = speakers.iter().map(|s| s.name.as_str()).collect();
        for line in &self.lines {
            if !speaker_names.contains(&line.speaker_id.as_str()) {
                return Err(format!(
                    "Speaker '{}' referenced in script but not found in configuration",
                    line.speaker_id
                ));
            }
        }

        // Check speaker count matches section requirements
        let unique_speakers: std::collections::HashSet<&str> = 
            self.lines.iter().map(|l| l.speaker_id.as_str()).collect();
        let required_count = self.section.required_speaker_count();
        if unique_speakers.len() != required_count {
            return Err(format!(
                "{} requires exactly {} speaker(s), but script has {}",
                self.section.description(),
                required_count,
                unique_speakers.len()
            ));
        }

        Ok(())
    }
}

/// The final audio output
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTrack {
    /// Audio format (e.g., "mp3", "wav")
    pub format: String,
    /// Actual duration of the audio
    pub duration: Duration,
    /// Path or URL to the file
    pub url: String,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}
