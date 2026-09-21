//! Audio artefacts and the programme that turns part recordings into one exam recording.

use serde::{Deserialize, Serialize};

use super::format::{ExamFormat, PlayCount};

/// Where a rendered recording lives and what it contains. Never the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTrack {
    /// Container name, e.g. "wav".
    pub container: String,
    pub sample_rate: u32,
    pub duration_ms: u32,
    /// Job id or server path, interpreted by the infrastructure layer.
    pub location: String,
}

/// Pause after the part announcement so candidates can read the questions.
pub const READING_PAUSE_MS: u32 = 20_000;
/// Pause between the first and second playing of a part.
pub const BETWEEN_PLAYS_MS: u32 = 8_000;
/// Pause after the last playing of a part.
pub const AFTER_PART_MS: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioSegment {
    /// Opening / closing music (optional asset; silence when absent).
    Music,
    /// The short sound before each recording.
    Tone,
    Silence { ms: u32 },
    /// Spoken by the announcer voice.
    Announcement(String),
    /// The synthesised passage of that part.
    Passage { part: u8 },
}

/// The ordered plan of the full exam recording, derived from the format's
/// playback rules. Rendering it is the infrastructure layer's job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioProgram {
    pub segments: Vec<AudioSegment>,
}

impl AudioProgram {
    pub fn for_format(format: &ExamFormat) -> Self {
        let mut segments = vec![
            AudioSegment::Music,
            AudioSegment::Announcement(format!(
                "This is the listening section of the test. There are {} parts. \
                 Parts played once will not be repeated. At the start of each recording you will hear a sound.",
                format.parts.len()
            )),
        ];
        for part in &format.parts {
            segments.push(AudioSegment::Tone);
            segments.push(AudioSegment::Announcement(format!(
                "{}. Listen to {} {} and do the tasks for questions {} to {}.",
                part.title,
                part.passage.label().to_lowercase(),
                match part.playback {
                    PlayCount::Once => "once",
                    PlayCount::Twice => "twice",
                },
                part.first_item(),
                part.last_item()
            )));
            segments.push(AudioSegment::Silence { ms: READING_PAUSE_MS });
            segments.push(AudioSegment::Passage { part: part.number });
            if part.playback == PlayCount::Twice {
                segments.push(AudioSegment::Silence { ms: BETWEEN_PLAYS_MS });
                segments.push(AudioSegment::Announcement(format!("Now you will hear {} again.", part.title)));
                segments.push(AudioSegment::Passage { part: part.number });
            }
            segments.push(AudioSegment::Silence { ms: AFTER_PART_MS });
        }
        segments.push(AudioSegment::Announcement(format!(
            "That is the end of the listening section. You now have {} minutes to check your answers.",
            format.check_minutes
        )));
        segments.push(AudioSegment::Silence { ms: u32::from(format.check_minutes) * 60_000 });
        segments.push(AudioSegment::Music);
        Self { segments }
    }

    /// Part numbers whose passage audio must exist before rendering.
    pub fn passages_needed(&self) -> Vec<u8> {
        let mut parts: Vec<u8> = self
            .segments
            .iter()
            .filter_map(|s| match s {
                AudioSegment::Passage { part } => Some(*part),
                _ => None,
            })
            .collect();
        parts.dedup();
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsg_program_plays_parts_three_and_four_twice() {
        let program = AudioProgram::for_format(&ExamFormat::hsg_national());
        let plays = |n: u8| program.segments.iter().filter(|s| **s == AudioSegment::Passage { part: n }).count();
        assert_eq!(plays(1), 1);
        assert_eq!(plays(3), 2);
        assert_eq!(program.passages_needed(), vec![1, 2, 3, 4]);
    }
}
