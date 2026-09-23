//! A passage is the text of one recording: ordered turns attributed to speaker labels.

use serde::{Deserialize, Serialize};

use super::error::DomainError;

/// Speaking rate used to estimate a script's duration before synthesis.
pub const WORDS_PER_MINUTE: f32 = 150.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// A `SpeakerConfig::label`, e.g. "Speaker A".
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passage {
    pub part: u8,
    pub topic: String,
    pub lines: Vec<Line>,
}

impl Passage {
    /// Parses "Speaker A: ..." text. Continuation lines belong to the previous
    /// turn; leading markdown decoration around a label is tolerated.
    pub fn parse(
        part: u8,
        topic: impl Into<String>,
        text: &str,
        labels: &[String],
    ) -> Result<Self, DomainError> {
        let mut lines: Vec<Line> = Vec::new();
        for raw in text.lines() {
            let trimmed = raw.trim().trim_start_matches(['*', '-', '#', '>', ' ']);
            if trimmed.is_empty() {
                continue;
            }
            let labelled = labels.iter().find_map(|label| {
                let rest = trimmed.strip_prefix(label.as_str())?;
                let rest = rest.trim_start_matches(['*', ' ']);
                let rest = rest.strip_prefix(':')?;
                Some((
                    label.clone(),
                    rest.trim_start_matches(['*', ' ']).trim().to_string(),
                ))
            });
            match (labelled, lines.last_mut()) {
                (Some((speaker, text)), _) => lines.push(Line { speaker, text }),
                (None, Some(previous)) => {
                    if !previous.text.is_empty() {
                        previous.text.push(' ');
                    }
                    previous.text.push_str(trimmed);
                }
                (None, None) => {
                    return Err(DomainError::InvalidPassage(format!(
                        "The script must start with a speaker label ({}); found: \"{}\"",
                        labels.join(", "),
                        truncate(trimmed, 60)
                    )));
                }
            }
        }
        if lines.is_empty() {
            return Err(DomainError::InvalidPassage(
                "The script is empty".to_string(),
            ));
        }
        Ok(Self {
            part,
            topic: topic.into(),
            lines,
        })
    }

    /// The canonical "Speaker A: ..." text, one turn per line. This is what is
    /// sent to text-to-speech and shown to the teacher.
    pub fn script_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| format!("{}: {}", l.speaker, l.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Spoken words only, used for grounding checks.
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn word_count(&self) -> usize {
        self.lines.iter().map(|l| count_words(&l.text)).sum()
    }

    pub fn estimated_minutes(&self) -> f32 {
        self.word_count() as f32 / WORDS_PER_MINUTE
    }

    /// Distinct labels in order of first appearance.
    pub fn speakers_used(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for line in &self.lines {
            if !seen.contains(&line.speaker) {
                seen.push(line.speaker.clone());
            }
        }
        seen
    }
}

pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max_chars).collect();
        format!("{cut}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Vec<String> {
        vec!["Speaker A".to_string(), "Speaker B".to_string()]
    }

    #[test]
    fn parses_turns_and_continuations() {
        let text =
            "**Speaker A:** Good morning.\nSpeaker B: Hello,\nhow are you?\n\nSpeaker A: Fine.";
        let passage = Passage::parse(1, "greeting", text, &labels()).unwrap();
        assert_eq!(passage.lines.len(), 3);
        assert_eq!(passage.lines[1].text, "Hello, how are you?");
        assert_eq!(passage.speakers_used(), labels());
        assert_eq!(passage.word_count(), 7);
    }

    #[test]
    fn rejects_unlabelled_start() {
        let err = Passage::parse(1, "x", "Hello there\nSpeaker A: hi", &labels()).unwrap_err();
        assert!(matches!(err, DomainError::InvalidPassage(_)));
    }
}
