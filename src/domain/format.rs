//! Exam formats: the blueprint a generated exam must follow.
//!
//! A format is data, not code. `ExamFormat::ielts_listening()` and
//! `ExamFormat::hsg_national()` are the two shipped presets; a teacher-defined
//! format is just another `ExamFormat` value.

use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::speaker::{Accent, Gender, SpeakerConfig, SpeakerRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormatId {
    /// IELTS Listening: 4 parts, 40 items, every recording played once.
    IeltsListening,
    /// Vietnamese national gifted-student exam (HSG Quoc gia), listening
    /// section: 4 parts, 35 items, parts 3-4 played twice.
    HsgNational,
}

impl FormatId {
    pub const ALL: [FormatId; 2] = [FormatId::IeltsListening, FormatId::HsgNational];

    pub fn key(self) -> &'static str {
        match self {
            FormatId::IeltsListening => "ielts",
            FormatId::HsgNational => "hsg",
        }
    }

    pub fn from_key(key: &str) -> Option<FormatId> {
        FormatId::ALL.into_iter().find(|f| f.key() == key)
    }

    pub fn format(self) -> ExamFormat {
        match self {
            FormatId::IeltsListening => ExamFormat::ielts_listening(),
            FormatId::HsgNational => ExamFormat::hsg_national(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayCount {
    Once,
    Twice,
}

impl PlayCount {
    pub fn label(self) -> &'static str {
        match self {
            PlayCount::Once => "ONCE",
            PlayCount::Twice => "TWICE",
        }
    }

    pub fn times(self) -> u8 {
        match self {
            PlayCount::Once => 1,
            PlayCount::Twice => 2,
        }
    }
}

/// What kind of recording a part is. Decides the speaker count and the
/// shape of the script prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassageKind {
    /// Two-way (or more) everyday or academic conversation.
    Conversation { speakers: u8 },
    /// A host interviewing `guests` guests (radio/news style).
    Interview { guests: u8 },
    /// One speaker: a talk, announcement or lecture.
    Monologue,
    /// "Part of a talk": one speaker, starts and ends mid-flow.
    Excerpt,
}

impl PassageKind {
    pub fn speaker_count(self) -> u8 {
        match self {
            PassageKind::Conversation { speakers } => speakers.max(2),
            PassageKind::Interview { guests } => guests + 1,
            PassageKind::Monologue | PassageKind::Excerpt => 1,
        }
    }

    pub fn is_dialogue(self) -> bool {
        self.speaker_count() > 1
    }

    pub fn label(self) -> String {
        match self {
            PassageKind::Conversation { speakers } => format!("Conversation ({speakers} speakers)"),
            PassageKind::Interview { guests } => format!("Interview (host + {guests} guests)"),
            PassageKind::Monologue => "Talk / monologue".to_string(),
            PassageKind::Excerpt => "Part of a talk".to_string(),
        }
    }
}

/// "NO MORE THAN TWO WORDS AND/OR A NUMBER".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordLimit {
    pub max_words: u8,
    pub allow_number: bool,
}

impl WordLimit {
    pub const fn words(max_words: u8) -> Self {
        Self {
            max_words,
            allow_number: false,
        }
    }

    pub const fn words_or_number(max_words: u8) -> Self {
        Self {
            max_words,
            allow_number: true,
        }
    }

    pub fn instruction(self) -> String {
        let words = match self.max_words {
            1 => "ONE WORD",
            2 => "TWO WORDS",
            3 => "THREE WORDS",
            n => return format!("NO MORE THAN {n} WORDS"),
        };
        if self.allow_number {
            format!("NO MORE THAN {words} AND/OR A NUMBER")
        } else {
            format!("NO MORE THAN {words}")
        }
    }
}

/// The question types this context knows how to generate and validate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    /// Statements answered T / F / NG.
    TrueFalseNotGiven,
    /// "Mentioned by guest S, guest A or Both (B)": one letter per item.
    WhoMentioned { guests: u8 },
    /// Choose `choose` letters out of `options` shared options (order-free).
    MultipleSelect { choose: u8, options: u8 },
    /// One stem, `options` options, one correct letter.
    MultipleChoice { options: u8 },
    /// Question answered with words taken from the recording.
    ShortAnswer(WordLimit),
    /// A summary paragraph with numbered gaps.
    SummaryCompletion(WordLimit),
    /// IELTS form / notes / table completion (numbered gaps in a structure).
    NoteCompletion(WordLimit),
    /// Sentence endings taken from the recording.
    SentenceCompletion(WordLimit),
    /// IELTS matching: items matched to a shared list of `options` letters.
    Matching { options: u8 },
}

impl TaskKind {
    pub fn label(&self) -> &'static str {
        match self {
            TaskKind::TrueFalseNotGiven => "True / False / Not Given",
            TaskKind::WhoMentioned { .. } => "Who mentioned it",
            TaskKind::MultipleSelect { .. } => "Multiple selection",
            TaskKind::MultipleChoice { .. } => "Multiple choice",
            TaskKind::ShortAnswer(_) => "Short answer",
            TaskKind::SummaryCompletion(_) => "Summary completion",
            TaskKind::NoteCompletion(_) => "Note / form completion",
            TaskKind::SentenceCompletion(_) => "Sentence completion",
            TaskKind::Matching { .. } => "Matching",
        }
    }

    pub fn word_limit(&self) -> Option<WordLimit> {
        match self {
            TaskKind::ShortAnswer(limit)
            | TaskKind::SummaryCompletion(limit)
            | TaskKind::NoteCompletion(limit)
            | TaskKind::SentenceCompletion(limit) => Some(*limit),
            _ => None,
        }
    }

    /// Number of options every item (or the task) offers, when letters are the answer.
    pub fn option_count(&self) -> Option<u8> {
        match self {
            TaskKind::WhoMentioned { guests } => Some(guests + 1),
            TaskKind::MultipleSelect { options, .. } => Some(*options),
            TaskKind::MultipleChoice { options } => Some(*options),
            TaskKind::Matching { options } => Some(*options),
            _ => None,
        }
    }

    /// True when the options are listed once for the whole task rather than per item.
    pub fn has_shared_options(&self) -> bool {
        matches!(
            self,
            TaskKind::WhoMentioned { .. }
                | TaskKind::MultipleSelect { .. }
                | TaskKind::Matching { .. }
        )
    }

    /// The rubric printed above the task, in the style of the reference paper.
    pub fn default_instruction(&self, first: u8, last: u8) -> String {
        let range = if first == last {
            format!("question {first}")
        } else {
            format!("questions {first} - {last}")
        };
        match self {
            TaskKind::TrueFalseNotGiven => format!(
                "For {range}, decide whether each of the following statements is True (T), False (F), or Not Given (NG) \
                 according to what you hear. Write T, F, or NG in the corresponding numbered boxes provided."
            ),
            TaskKind::WhoMentioned { .. } => format!(
                "For {range}, decide whether the following are mentioned by only one of the guests, or by both of them. \
                 Write the corresponding letter in the numbered boxes provided."
            ),
            TaskKind::MultipleSelect { choose, options } => {
                let last_letter = (b'A' + options.saturating_sub(1)) as char;
                format!(
                    "For {range}, choose {} letters from A-{last_letter} to indicate {} true statements according to the recording. \
                     Write your answers in the corresponding numbered boxes provided.",
                    number_word(*choose),
                    number_word(*choose)
                )
            }
            TaskKind::MultipleChoice { options } => {
                let last_letter = (b'A' + options.saturating_sub(1)) as char;
                format!(
                    "For {range}, write the letter A, B, C or {last_letter} in the corresponding numbered boxes provided to indicate \
                     the correct answer to each of the following questions according to what is stated or implied by the speaker."
                )
            }
            TaskKind::ShortAnswer(limit) => format!(
                "For {range}, answer each of the following questions with {} taken from the recording. \
                 Write your answers in the corresponding spaces provided.",
                limit.instruction()
            ),
            TaskKind::SummaryCompletion(limit) => format!(
                "For {range}, complete the following summary with {} taken from the recording for each space. \
                 Write your answers in the corresponding numbered boxes provided.",
                limit.instruction()
            ),
            TaskKind::NoteCompletion(limit) => {
                format!(
                    "Complete the notes below. Write {} for each answer ({range}).",
                    limit.instruction()
                )
            }
            TaskKind::SentenceCompletion(limit) => {
                format!(
                    "Complete the sentences below. Write {} for each answer ({range}).",
                    limit.instruction()
                )
            }
            TaskKind::Matching { .. } => format!(
                "For {range}, choose the correct letter from the list of options and write it next to each question."
            ),
        }
    }
}

fn number_word(n: u8) -> String {
    match n {
        1 => "ONE".to_string(),
        2 => "TWO".to_string(),
        3 => "THREE".to_string(),
        4 => "FOUR".to_string(),
        5 => "FIVE".to_string(),
        n => n.to_string(),
    }
}

/// A task inside a part: one question type over a contiguous item range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub kind: TaskKind,
    pub first: u8,
    pub last: u8,
}

impl TaskSpec {
    pub fn new(kind: TaskKind, first: u8, last: u8) -> Self {
        Self { kind, first, last }
    }

    pub fn count(&self) -> u8 {
        self.last.saturating_sub(self.first) + 1
    }

    pub fn range_label(&self) -> String {
        if self.first == self.last {
            self.first.to_string()
        } else {
            format!("{} - {}", self.first, self.last)
        }
    }

    pub fn instruction(&self) -> String {
        self.kind.default_instruction(self.first, self.last)
    }
}

/// One part of the exam: a recording plus its tasks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartSpec {
    pub number: u8,
    pub title: String,
    pub passage: PassageKind,
    /// Genre guidance for the script prompt ("a radio interview with two guests...").
    pub brief: String,
    pub playback: PlayCount,
    pub min_minutes: f32,
    pub max_minutes: f32,
    pub default_speakers: Vec<SpeakerConfig>,
    pub tasks: Vec<TaskSpec>,
}

impl PartSpec {
    pub fn first_item(&self) -> u8 {
        self.tasks.first().map(|t| t.first).unwrap_or(0)
    }

    pub fn last_item(&self) -> u8 {
        self.tasks.last().map(|t| t.last).unwrap_or(0)
    }

    pub fn item_count(&self) -> u8 {
        self.tasks.iter().map(TaskSpec::count).sum()
    }

    pub fn speaker_count(&self) -> u8 {
        self.passage.speaker_count()
    }

    pub fn duration_label(&self) -> String {
        format!("{:.1}-{:.1} minutes", self.min_minutes, self.max_minutes)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExamFormat {
    pub id: FormatId,
    pub name: String,
    pub total_points: f32,
    /// Length of the whole listening section including pauses and replays.
    pub listening_minutes: u16,
    /// Time given at the end to check answers.
    pub check_minutes: u8,
    pub parts: Vec<PartSpec>,
}

impl ExamFormat {
    pub fn part(&self, number: u8) -> Option<&PartSpec> {
        self.parts.iter().find(|p| p.number == number)
    }

    pub fn total_items(&self) -> u8 {
        self.parts.iter().map(PartSpec::item_count).sum()
    }

    /// Item numbering must be contiguous across the whole exam and every
    /// part must ship the right number of default voices.
    pub fn check_consistency(&self) -> Result<(), DomainError> {
        let mut expected = 1u8;
        for part in &self.parts {
            if part.tasks.is_empty() {
                return Err(DomainError::InvalidRequest(format!(
                    "Part {} has no tasks",
                    part.number
                )));
            }
            if part.default_speakers.len() != part.speaker_count() as usize {
                return Err(DomainError::InvalidRequest(format!(
                    "Part {} needs {} default speakers, found {}",
                    part.number,
                    part.speaker_count(),
                    part.default_speakers.len()
                )));
            }
            for task in &part.tasks {
                if task.first != expected || task.last < task.first {
                    return Err(DomainError::InvalidRequest(format!(
                        "Part {}: task {:?} starts at {} but {} was expected",
                        part.number, task.kind, task.first, expected
                    )));
                }
                expected = task.last + 1;
            }
        }
        Ok(())
    }

    /// IELTS Listening as sat today: 4 parts, 40 items, every recording once.
    /// The task plan is the common shape; teachers may swap task kinds per part.
    pub fn ielts_listening() -> Self {
        let one_word = WordLimit::words_or_number(1);
        Self {
            id: FormatId::IeltsListening,
            name: "IELTS Listening".to_string(),
            total_points: 40.0,
            listening_minutes: 30,
            check_minutes: 10,
            parts: vec![
                PartSpec {
                    number: 1,
                    title: "Part 1".to_string(),
                    passage: PassageKind::Conversation { speakers: 2 },
                    brief: "A transactional conversation between two people in an everyday social context \
                            (booking, enquiring about a service, arranging travel or accommodation)."
                        .to_string(),
                    playback: PlayCount::Once,
                    min_minutes: 2.5,
                    max_minutes: 3.5,
                    default_speakers: vec![
                        SpeakerConfig::new("Speaker A", Gender::Female, Accent::British, SpeakerRole::Receptionist),
                        SpeakerConfig::new("Speaker B", Gender::Male, Accent::American, SpeakerRole::Other("Customer".into())),
                    ],
                    tasks: vec![TaskSpec::new(TaskKind::NoteCompletion(one_word), 1, 10)],
                },
                PartSpec {
                    number: 2,
                    title: "Part 2".to_string(),
                    passage: PassageKind::Monologue,
                    brief: "A monologue in an everyday social context: a talk about local facilities, \
                            an event, a tour or arrangements."
                        .to_string(),
                    playback: PlayCount::Once,
                    min_minutes: 3.0,
                    max_minutes: 4.0,
                    default_speakers: vec![SpeakerConfig::new("Speaker A", Gender::Male, Accent::British, SpeakerRole::Guide)],
                    tasks: vec![
                        TaskSpec::new(TaskKind::MultipleChoice { options: 3 }, 11, 15),
                        TaskSpec::new(TaskKind::Matching { options: 7 }, 16, 20),
                    ],
                },
                PartSpec {
                    number: 3,
                    title: "Part 3".to_string(),
                    passage: PassageKind::Conversation { speakers: 2 },
                    brief: "A conversation in an educational or training context: a tutor and a student \
                            discussing an assignment, or students planning a project."
                        .to_string(),
                    playback: PlayCount::Once,
                    min_minutes: 3.5,
                    max_minutes: 4.5,
                    default_speakers: vec![
                        SpeakerConfig::new("Speaker A", Gender::Female, Accent::Australian, SpeakerRole::Student),
                        SpeakerConfig::new("Speaker B", Gender::Male, Accent::British, SpeakerRole::Professor),
                    ],
                    tasks: vec![
                        TaskSpec::new(TaskKind::MultipleChoice { options: 3 }, 21, 26),
                        TaskSpec::new(TaskKind::Matching { options: 6 }, 27, 30),
                    ],
                },
                PartSpec {
                    number: 4,
                    title: "Part 4".to_string(),
                    passage: PassageKind::Monologue,
                    brief: "A university-style lecture on an academic subject, delivered to a general audience."
                        .to_string(),
                    playback: PlayCount::Once,
                    min_minutes: 4.0,
                    max_minutes: 5.0,
                    default_speakers: vec![SpeakerConfig::new("Speaker A", Gender::Male, Accent::American, SpeakerRole::Professor)],
                    tasks: vec![TaskSpec::new(TaskKind::NoteCompletion(one_word), 31, 40)],
                },
            ],
        }
    }

    /// The national gifted-student exam listening section, transcribed from the
    /// official 2025-2026 paper: 35 items, 5.0 points, 30 minutes, parts 1-2
    /// played once, parts 3-4 twice, two minutes to check at the end.
    pub fn hsg_national() -> Self {
        Self {
            id: FormatId::HsgNational,
            name: "HSG Quoc gia - Listening".to_string(),
            total_points: 5.0,
            listening_minutes: 30,
            check_minutes: 2,
            parts: vec![
                PartSpec {
                    number: 1,
                    title: "Part 1".to_string(),
                    passage: PassageKind::Interview { guests: 2 },
                    brief: "A radio or podcast conversation: a host interviews two guests with different \
                            perspectives on a current-affairs topic. Both guests must contribute distinct \
                            claims, and some points must be raised by both."
                        .to_string(),
                    playback: PlayCount::Once,
                    min_minutes: 4.0,
                    max_minutes: 5.0,
                    default_speakers: vec![
                        SpeakerConfig::new("Speaker A", Gender::Female, Accent::British, SpeakerRole::Host),
                        SpeakerConfig::new("Speaker B", Gender::Female, Accent::American, SpeakerRole::Guest),
                        SpeakerConfig::new("Speaker C", Gender::Male, Accent::British, SpeakerRole::Expert),
                    ],
                    tasks: vec![
                        TaskSpec::new(TaskKind::TrueFalseNotGiven, 1, 5),
                        TaskSpec::new(TaskKind::WhoMentioned { guests: 2 }, 6, 10),
                    ],
                },
                PartSpec {
                    number: 2,
                    title: "Part 2".to_string(),
                    passage: PassageKind::Monologue,
                    brief: "A news feature or documentary-style talk by one presenter, dense with facts, \
                            figures, comparisons and named examples."
                        .to_string(),
                    playback: PlayCount::Once,
                    min_minutes: 5.0,
                    max_minutes: 6.0,
                    default_speakers: vec![SpeakerConfig::new("Speaker A", Gender::Male, Accent::British, SpeakerRole::Reporter)],
                    tasks: vec![
                        TaskSpec::new(TaskKind::MultipleSelect { choose: 2, options: 5 }, 11, 12),
                        TaskSpec::new(TaskKind::MultipleSelect { choose: 3, options: 7 }, 13, 15),
                        TaskSpec::new(TaskKind::MultipleChoice { options: 4 }, 16, 20),
                    ],
                },
                PartSpec {
                    number: 3,
                    title: "Part 3".to_string(),
                    passage: PassageKind::Excerpt,
                    brief: "Part of a talk by one speaker on a science, health or lifestyle topic, \
                            with precise terms a listener can write down."
                        .to_string(),
                    playback: PlayCount::Twice,
                    min_minutes: 2.5,
                    max_minutes: 3.5,
                    default_speakers: vec![SpeakerConfig::new("Speaker A", Gender::Female, Accent::American, SpeakerRole::Expert)],
                    tasks: vec![TaskSpec::new(TaskKind::ShortAnswer(WordLimit::words(2)), 21, 25)],
                },
                PartSpec {
                    number: 4,
                    title: "Part 4".to_string(),
                    passage: PassageKind::Excerpt,
                    brief: "Part of a talk by one speaker on a history, culture or technology topic, \
                            structured in clear sub-sections so a summary can follow it."
                        .to_string(),
                    playback: PlayCount::Twice,
                    min_minutes: 3.5,
                    max_minutes: 4.5,
                    default_speakers: vec![SpeakerConfig::new("Speaker A", Gender::Male, Accent::British, SpeakerRole::Narrator)],
                    tasks: vec![TaskSpec::new(TaskKind::SummaryCompletion(WordLimit::words_or_number(1)), 26, 35)],
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_consistent() {
        for id in FormatId::ALL {
            id.format().check_consistency().expect("preset numbering");
        }
        assert_eq!(ExamFormat::ielts_listening().total_items(), 40);
        assert_eq!(ExamFormat::hsg_national().total_items(), 35);
    }

    #[test]
    fn hsg_playback_matches_the_paper() {
        let hsg = ExamFormat::hsg_national();
        assert_eq!(hsg.part(1).unwrap().playback, PlayCount::Once);
        assert_eq!(hsg.part(3).unwrap().playback, PlayCount::Twice);
        assert_eq!(hsg.part(1).unwrap().speaker_count(), 3);
    }

    #[test]
    fn word_limit_wording() {
        assert_eq!(WordLimit::words(2).instruction(), "NO MORE THAN TWO WORDS");
        assert_eq!(
            WordLimit::words_or_number(1).instruction(),
            "NO MORE THAN ONE WORD AND/OR A NUMBER"
        );
    }
}
