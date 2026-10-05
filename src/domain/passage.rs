//! A passage is the text of one recording: ordered turns attributed to speaker labels.

use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::speaker::SpeakerConfig;
use super::speech;
use super::voice::{SpeakerChange, speaker_change};

/// Speaking rate used to estimate a script's duration before synthesis.
pub const WORDS_PER_MINUTE: f32 = 150.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// A `SpeakerConfig::label`, e.g. "Speaker A".
    pub speaker: String,
    /// The spoken words, plus any speech markup (`<sigh>`, see `speech`).
    pub text: String,
}

impl Line {
    pub fn new(speaker: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            speaker: speaker.into(),
            text: text.into(),
        }
    }

    /// What a listener hears: the text without speech markup.
    pub fn display_text(&self) -> String {
        speech::display_text(&self.text)
    }

    /// What the speech model gets (`speech::speech_text`).
    pub fn speech_text(&self, backchannels: bool) -> String {
        speech::speech_text(&self.text, backchannels)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passage {
    pub part: u8,
    pub topic: String,
    pub lines: Vec<Line>,
    /// The line-up the script was written for (`for_speakers`): names,
    /// pronouns and wording follow it. Empty for scripts made before 0.8,
    /// which are then never out of date (`speakers_changed`).
    #[serde(default)]
    pub written_for: Vec<SpeakerConfig>,
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
            written_for: Vec::new(),
        })
    }

    /// This passage, noted as written for `speakers` ("Written for"). Set
    /// when a script is generated, and again when the teacher keeps a script
    /// after editing its speakers.
    pub fn for_speakers(self, speakers: &[SpeakerConfig]) -> Self {
        Self {
            written_for: speakers.to_vec(),
            ..self
        }
    }

    /// What the `current` speakers make out of date compared with the
    /// line-up the script was written for (`speaker_change`). Nothing when
    /// `written_for` is empty: a script from before 0.8 has nothing to compare.
    pub fn speakers_changed(&self, current: &[SpeakerConfig]) -> SpeakerChange {
        speaker_change(&self.written_for, current)
    }

    /// The canonical "Speaker A: ..." text, one turn per line, speech tags
    /// included: what the teacher sees on screen. Text-to-speech gets the
    /// lines without labels (Gemini reads its input verbatim).
    pub fn script_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| format!("{}: {}", l.speaker, l.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// "Speaker A: ..." lines with the words a listener hears and no speech
    /// markup: what question prompts quote and what is downloaded and printed
    /// as the transcript. A turn that is only markup is left out.
    pub fn transcript_text(&self) -> String {
        self.lines
            .iter()
            .filter_map(|l| {
                let words = l.display_text();
                (!words.is_empty()).then(|| format!("{}: {words}", l.speaker))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Spoken words only, without speech markup: the grounding haystack.
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(Line::display_text)
            .filter(|words| !words.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Words a listener hears; speech tags never count.
    pub fn word_count(&self) -> usize {
        self.lines
            .iter()
            .map(|l| count_words(&l.display_text()))
            .sum()
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
    fn text_views_keep_or_drop_speech_markup() {
        let text = "Speaker A: Well <sigh>, I suppose |mhm| so.\nSpeaker B: <laugh>\nSpeaker B: Fine [music] by me!";
        let passage = Passage::parse(1, "markup", text, &labels()).unwrap();
        // The teacher's view keeps the tags; parse keeps them in the line.
        assert_eq!(passage.script_text(), text);
        assert_eq!(passage.lines[0].text, "Well <sigh>, I suppose |mhm| so.");
        assert_eq!(
            passage.transcript_text(),
            "Speaker A: Well, I suppose so.\nSpeaker B: Fine by me!"
        );
        assert_eq!(passage.plain_text(), "Well, I suppose so. Fine by me!");
        assert_eq!(passage.word_count(), 7);
        assert_eq!(
            passage.lines[0].speech_text(false),
            "Well <sigh>, I suppose so."
        );
        assert_eq!(
            passage.lines[0].speech_text(true),
            "Well <sigh>, I suppose |mhm| so."
        );
        let line = Line::new("Speaker A", "Hi <laugh> there.");
        assert_eq!(
            (line.speaker.as_str(), line.display_text().as_str()),
            ("Speaker A", "Hi there.")
        );
    }

    #[test]
    fn rejects_unlabelled_start() {
        let err = Passage::parse(1, "x", "Hello there\nSpeaker A: hi", &labels()).unwrap_err();
        assert!(matches!(err, DomainError::InvalidPassage(_)));
    }

    fn line_up() -> Vec<SpeakerConfig> {
        use crate::domain::speaker::{Accent, Gender, SpeakerRole};
        vec![
            SpeakerConfig::new(
                "Speaker A",
                Gender::Female,
                Accent::British,
                SpeakerRole::Host,
            ),
            SpeakerConfig::new("Speaker B", Gender::Male, Accent::Irish, SpeakerRole::Guest),
        ]
    }

    #[test]
    fn a_passage_remembers_who_it_was_written_for() {
        let text = "Speaker A: Hello.\nSpeaker B: Hi.";
        let parsed = Passage::parse(1, "greeting", text, &labels()).unwrap();
        assert!(parsed.written_for.is_empty());
        let passage = parsed.clone().for_speakers(&line_up());
        assert_eq!(passage.written_for, line_up());
        assert_eq!(passage.lines, parsed.lines);
        assert_eq!(
            passage.speakers_changed(&line_up()),
            SpeakerChange::default()
        );

        let mut edited = line_up();
        edited[1].role = crate::domain::speaker::SpeakerRole::Expert;
        assert!(passage.speakers_changed(&edited).script);
        // Kept for the edited line-up, it is up to date again.
        let kept = passage.for_speakers(&edited);
        assert_eq!(kept.speakers_changed(&edited), SpeakerChange::default());
    }

    #[test]
    fn passages_saved_before_written_for_still_load() {
        let passage: Passage = serde_json::from_str(
            r#"{"part": 1, "topic": "greeting", "lines": [{"speaker": "Speaker A", "text": "Hello."}]}"#,
        )
        .unwrap();
        assert!(passage.written_for.is_empty());
        let mut edited = line_up();
        edited[0].gender = crate::domain::speaker::Gender::Male;
        // Nothing to compare with: never out of date.
        assert_eq!(passage.speakers_changed(&edited), SpeakerChange::default());
        assert_eq!(passage.speakers_changed(&[]), SpeakerChange::default());
    }
}
