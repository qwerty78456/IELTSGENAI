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

    /// Parses a script the teacher edited, one "Speaker A: ..." turn per line
    /// as `script_text` shows it. Stricter than `parse`, which tolerates model
    /// output: a line that starts like a speaker label but names no speaker
    /// of this part ("speaker b:", "Speaker C:" in a dialogue of two), or a
    /// label without its colon, is refused with its line number instead of
    /// being joined to the previous turn and read aloud.
    pub fn parse_edited(
        part: u8,
        topic: impl Into<String>,
        text: &str,
        labels: &[String],
    ) -> Result<Self, DomainError> {
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim().trim_start_matches(['*', '-', '#', '>', ' ']);
            if let Some(problem) = edited_label_problem(line, labels) {
                return Err(DomainError::InvalidPassage(format!(
                    "Line {}: {problem}",
                    index + 1
                )));
            }
        }
        Self::parse(part, topic, text, labels)
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

    /// Why the script is too big to send to a language model or the voices,
    /// if it is: too many turns, words, or characters (one huge "word").
    /// Real scripts stay far below these limits.
    pub fn size_problem(&self) -> Option<String> {
        if self.lines.len() > MAX_PASSAGE_LINES {
            return Some(format!(
                "The script is too long: at most {MAX_PASSAGE_LINES} turns, found {}",
                self.lines.len()
            ));
        }
        let words = self.word_count();
        if words > MAX_PASSAGE_WORDS {
            return Some(format!(
                "The script is too long: at most {MAX_PASSAGE_WORDS} words, found {words}"
            ));
        }
        let chars: usize = self
            .lines
            .iter()
            .map(|l| l.speaker.chars().count() + l.text.chars().count())
            .sum();
        if chars > MAX_PASSAGE_CHARS {
            return Some(format!(
                "The script is too long: at most {MAX_PASSAGE_CHARS} characters, found {chars}"
            ));
        }
        None
    }
}

/// What is wrong with the label of one edited line, if anything
/// (`Passage::parse_edited`).
fn edited_label_problem(line: &str, labels: &[String]) -> Option<String> {
    // "Speaker A or Speaker B", "Speaker A, Speaker B or Speaker C".
    let speakers = match labels.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    };
    if let Some((head, _)) = line.split_once(':') {
        let head = head.trim_matches(['*', ' ']);
        if labels.iter().any(|label| label == head) {
            return None;
        }
        if looks_like_label(head) {
            return Some(format!(
                "\"{head}:\" is not a speaker of this part; start each turn with {speakers}:"
            ));
        }
    }
    for label in labels {
        let Some(rest) = line.strip_prefix(label.as_str()) else {
            continue;
        };
        // "Speaker AB" is not "Speaker A".
        if rest.chars().next().is_some_and(char::is_alphanumeric) {
            continue;
        }
        if !rest.trim_start_matches(['*', ' ']).starts_with(':') {
            return Some(format!("put a colon after \"{label}\""));
        }
    }
    None
}

/// "Speaker B", "speaker b", "SpeakerC", "Speaker 2": a speaker label in any spelling.
fn looks_like_label(head: &str) -> bool {
    let compact: String = head
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    compact.strip_prefix("speaker").is_some_and(|rest| {
        (1..=3).contains(&rest.chars().count()) && rest.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/// Size limits of one script (`Passage::size_problem`).
pub const MAX_PASSAGE_LINES: usize = 200;
pub const MAX_PASSAGE_WORDS: usize = 3_000;
pub const MAX_PASSAGE_CHARS: usize = 30_000;

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

    fn edit_error(text: &str) -> String {
        Passage::parse_edited(1, "topic", text, &labels())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn edited_text_with_an_unknown_label_is_refused() {
        let error = edit_error("Speaker A: Hi.\nspeaker b: Yes, that's right.");
        assert!(error.contains("Line 2"), "{error}");
        assert!(error.contains("\"speaker b:\" is not a speaker"), "{error}");
        assert!(error.contains("Speaker A or Speaker B:"), "{error}");
        let error = edit_error("Speaker A: Hi.\nSpeaker B: Hello.\n**Speaker C:** And me.");
        assert!(error.starts_with("Line 3: \"Speaker C:\""), "{error}");
        let three = ["Speaker A", "Speaker B", "Speaker C"].map(String::from);
        let error = Passage::parse_edited(1, "t", "Speaker A: Hi.\nSpeaker D: Me.", &three)
            .unwrap_err()
            .to_string();
        assert!(
            error.ends_with("start each turn with Speaker A, Speaker B or Speaker C:"),
            "{error}"
        );
        // `parse` would have joined it to the previous turn.
        let lenient = Passage::parse(1, "t", "Speaker A: Hi.\nSpeaker C: Me.", &labels()).unwrap();
        assert_eq!(lenient.lines.len(), 1);
    }

    #[test]
    fn edited_text_needs_the_colon_after_a_label() {
        let error = edit_error("Speaker A: Hi.\nSpeaker B Yes.");
        assert_eq!(error, "Line 2: put a colon after \"Speaker B\"");
        assert!(edit_error("Speaker A").contains("put a colon"));
    }

    #[test]
    fn edited_continuation_lines_like_times_are_kept() {
        let passage = Passage::parse_edited(
            1,
            "tour",
            "Speaker A: The tour starts at\nTime: ten sharp.\nSpeaker B: Great.",
            &labels(),
        )
        .unwrap();
        assert_eq!(passage.lines.len(), 2);
        assert_eq!(passage.lines[0].text, "The tour starts at Time: ten sharp.");
    }

    #[test]
    fn empty_edited_text_is_refused() {
        for empty in ["", "   \n  "] {
            assert_eq!(edit_error(empty), "The script is empty");
        }
    }

    #[test]
    fn edited_text_parses_like_generated_text() {
        let generated = Passage::parse(
            1,
            "topic",
            "Speaker A: Hello <sigh> there.\nSpeaker B: Hi |mhm| yes.",
            &labels(),
        )
        .unwrap();
        let edited =
            Passage::parse_edited(1, "topic", &generated.script_text(), &labels()).unwrap();
        assert_eq!(edited, generated);
    }
}
