//! Invariants checked before anything reaches the teacher.
//!
//! Validation never mutates; it returns a list of issues so the UI can show
//! them next to the draft. `Severity::Error` means the item is unusable as a
//! key; `Severity::Warning` means a teacher should look at it.

use serde::{Deserialize, Serialize};

use super::format::{PartSpec, TaskKind};
use super::passage::{count_words, Passage};
use super::speaker::SpeakerConfig;
use super::task::{Answer, Choice, Item, Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: Severity,
    /// Item number when the issue is about one item.
    pub item: Option<u8>,
    pub message: String,
}

impl ValidationIssue {
    fn error(item: Option<u8>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, item, message: message.into() }
    }

    fn warning(item: Option<u8>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, item, message: message.into() }
    }

    pub fn display(&self) -> String {
        let prefix = match self.severity {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
        };
        match self.item {
            Some(n) => format!("{prefix} (Q{n}): {}", self.message),
            None => format!("{prefix}: {}", self.message),
        }
    }
}

pub fn has_errors(issues: &[ValidationIssue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

/// The speaker line-up must match the part's passage kind and use unique labels.
pub fn validate_speakers(spec: &PartSpec, speakers: &[SpeakerConfig]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let expected = spec.speaker_count() as usize;
    if speakers.len() != expected {
        issues.push(ValidationIssue::error(
            None,
            format!("{} needs exactly {} speaker(s), found {}", spec.title, expected, speakers.len()),
        ));
    }
    for (i, speaker) in speakers.iter().enumerate() {
        if speakers[..i].iter().any(|other| other.label == speaker.label) {
            issues.push(ValidationIssue::error(None, format!("Duplicate speaker label \"{}\"", speaker.label)));
        }
    }
    issues
}

/// A passage must use only the configured labels, all of them, and fit the
/// part's duration window (by estimate).
pub fn validate_passage(passage: &Passage, spec: &PartSpec, speakers: &[SpeakerConfig]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let labels: Vec<&str> = speakers.iter().map(|s| s.label.as_str()).collect();
    let used = passage.speakers_used();
    for label in &used {
        if !labels.contains(&label.as_str()) {
            issues.push(ValidationIssue::error(None, format!("Unknown speaker label \"{label}\" in the script")));
        }
    }
    for label in &labels {
        if !used.iter().any(|u| u == label) {
            issues.push(ValidationIssue::warning(None, format!("{label} never speaks")));
        }
    }
    let minutes = passage.estimated_minutes();
    if minutes < spec.min_minutes * 0.8 {
        issues.push(ValidationIssue::warning(
            None,
            format!("Script is short: about {minutes:.1} min, {} expects {}", spec.title, spec.duration_label()),
        ));
    } else if minutes > spec.max_minutes * 1.25 {
        issues.push(ValidationIssue::warning(
            None,
            format!("Script is long: about {minutes:.1} min, {} expects {}", spec.title, spec.duration_label()),
        ));
    }
    if passage.lines.iter().any(|l| l.text.contains("___") || l.text.contains("[FILL")) {
        issues.push(ValidationIssue::error(None, "The script contains gaps; scripts must be complete"));
    }
    issues
}

/// A task must follow its spec exactly and every key must be grounded in the passage.
pub fn validate_task(task: &Task, passage: Option<&Passage>) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let spec = &task.spec;
    let expected: Vec<u8> = (spec.first..=spec.last).collect();
    let actual: Vec<u8> = task.items.iter().map(|i| i.number).collect();
    if actual != expected {
        issues.push(ValidationIssue::error(
            None,
            format!("Items must be numbered {} to {} in order, found {:?}", spec.first, spec.last, actual),
        ));
    }
    if spec.kind.has_shared_options() {
        let expected_options = spec.kind.option_count().unwrap_or(0) as usize;
        if task.shared_options.len() != expected_options {
            issues.push(ValidationIssue::error(
                None,
                format!(
                    "{} needs {} shared options, found {}",
                    spec.kind.label(),
                    expected_options,
                    task.shared_options.len()
                ),
            ));
        }
        if matches!(spec.kind, TaskKind::WhoMentioned { .. }) {
            issues.extend(check_distinct_letters(&task.shared_options));
        } else {
            issues.extend(check_lettering(&task.shared_options, None));
        }
    }
    if let TaskKind::SummaryCompletion(_) = spec.kind {
        match &task.summary {
            None => issues.push(ValidationIssue::error(None, "Summary completion needs the summary paragraph")),
            Some(summary) => {
                for number in &expected {
                    if !summary.contains(&format!("({number})")) {
                        issues.push(ValidationIssue::error(Some(*number), format!("The summary has no gap ({number})")));
                    }
                }
            }
        }
    }
    let haystack = passage.map(|p| normalize(&p.plain_text()));
    for item in &task.items {
        issues.extend(validate_item(item, task, haystack.as_deref()));
    }
    if let TaskKind::MultipleSelect { choose, .. } = spec.kind {
        let mut letters: Vec<char> = task.items.iter().flat_map(|i| i.answer.letters().iter().copied()).collect();
        let total = letters.len();
        letters.sort_unstable();
        letters.dedup();
        if total != choose as usize || letters.len() != total {
            issues.push(ValidationIssue::error(
                None,
                format!("Multiple selection must have {choose} distinct correct letters across its items"),
            ));
        }
    }
    issues
}

fn validate_item(item: &Item, task: &Task, haystack: Option<&str>) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let number = Some(item.number);
    let kind = &task.spec.kind;
    let is_gap = matches!(
        kind,
        TaskKind::SummaryCompletion(_) | TaskKind::NoteCompletion(_) | TaskKind::MultipleSelect { .. }
    );
    if item.stem.trim().is_empty() && !is_gap {
        issues.push(ValidationIssue::error(number, "Empty question"));
    }
    match kind {
        TaskKind::TrueFalseNotGiven => {
            if !matches!(item.answer, Answer::Tfng(_)) {
                issues.push(ValidationIssue::error(number, "Answer must be T, F or NG"));
            }
        }
        TaskKind::MultipleChoice { options } => {
            if item.options.len() != *options as usize {
                issues.push(ValidationIssue::error(number, format!("Needs {options} options, found {}", item.options.len())));
            }
            issues.extend(check_lettering(&item.options, number));
            issues.extend(check_letter_answer(item, &item.options, 1));
        }
        TaskKind::WhoMentioned { .. } | TaskKind::Matching { .. } | TaskKind::MultipleSelect { .. } => {
            issues.extend(check_letter_answer(item, &task.shared_options, 1));
        }
        TaskKind::ShortAnswer(limit)
        | TaskKind::SummaryCompletion(limit)
        | TaskKind::NoteCompletion(limit)
        | TaskKind::SentenceCompletion(limit) => match &item.answer {
            Answer::Text(variants) if !variants.is_empty() => {
                for variant in variants {
                    let words = count_words(variant);
                    if words == 0 {
                        issues.push(ValidationIssue::error(number, "Empty answer"));
                    } else if words > limit.max_words as usize {
                        issues.push(ValidationIssue::error(
                            number,
                            format!("\"{variant}\" has {words} words; the limit is {}", limit.instruction()),
                        ));
                    }
                    if !limit.allow_number && is_number(variant) {
                        issues.push(ValidationIssue::error(
                            number,
                            format!("\"{variant}\" is a number but numbers are not allowed"),
                        ));
                    }
                }
                if let Some(haystack) = haystack {
                    if !variants.iter().any(|v| haystack.contains(&normalize(v))) {
                        issues.push(ValidationIssue::error(
                            number,
                            format!("Key \"{}\" does not occur in the script", variants[0]),
                        ));
                    }
                }
            }
            _ => issues.push(ValidationIssue::error(number, "Answer must be text taken from the recording")),
        },
    }
    if let Some(haystack) = haystack {
        if item.evidence.trim().is_empty() {
            issues.push(ValidationIssue::warning(number, "No evidence quoted from the script"));
        } else if !haystack.contains(&normalize(&item.evidence)) {
            issues.push(ValidationIssue::warning(number, "The quoted evidence is not verbatim from the script"));
        }
    }
    issues
}

fn check_lettering(options: &[Choice], item: Option<u8>) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for (i, option) in options.iter().enumerate() {
        let expected = (b'A' + i as u8) as char;
        if option.letter != expected {
            issues.push(ValidationIssue::error(item, format!("Options must be lettered A, B, C...; found {}", option.letter)));
            break;
        }
        if option.text.trim().is_empty() {
            issues.push(ValidationIssue::error(item, format!("Option {} is empty", option.letter)));
        }
    }
    issues
}

/// Who-mentioned options carry initials ("S", "A") plus "B" for both, so only
/// distinctness is required, not alphabetical order.
fn check_distinct_letters(options: &[Choice]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for (i, option) in options.iter().enumerate() {
        if options[..i].iter().any(|o| o.letter == option.letter) {
            issues.push(ValidationIssue::error(None, format!("Option letter {} is used twice", option.letter)));
        }
        if option.text.trim().is_empty() {
            issues.push(ValidationIssue::error(None, format!("Option {} is empty", option.letter)));
        }
    }
    issues
}

fn check_letter_answer(item: &Item, options: &[Choice], expected_count: usize) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    match &item.answer {
        Answer::Letters(letters) => {
            if letters.len() != expected_count {
                issues.push(ValidationIssue::error(
                    Some(item.number),
                    format!("Expected {expected_count} letter(s) as the answer"),
                ));
            }
            for letter in letters {
                if !options.iter().any(|o| o.letter == *letter) {
                    issues.push(ValidationIssue::error(Some(item.number), format!("Answer {letter} is not one of the options")));
                }
            }
        }
        _ => issues.push(ValidationIssue::error(Some(item.number), "Answer must be a letter")),
    }
    issues
}

/// Lower-case, punctuation stripped, apostrophes removed, single spaces: the
/// comparison form used for grounding checks.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(c.to_lowercase());
        } else if c == '\'' || c == '\u{2019}' {
            continue;
        } else {
            pending_space = true;
        }
    }
    out
}

fn is_number(text: &str) -> bool {
    let stripped: String = text.chars().filter(|c| !matches!(c, ',' | '.' | '%' | ' ')).collect();
    !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::format::{ExamFormat, TaskSpec, WordLimit};
    use crate::domain::task::Tfng;

    fn passage() -> Passage {
        Passage::parse(
            3,
            "retro-walking",
            "Speaker A: Walking backwards, or retro-walking, can ease knee pain and improve balance. It is odd at first.",
            &["Speaker A".to_string()],
        )
        .unwrap()
    }

    fn short_answer_task(answer: &str) -> Task {
        Task {
            spec: TaskSpec::new(TaskKind::ShortAnswer(WordLimit::words(2)), 21, 21),
            instruction: String::new(),
            shared_options: vec![],
            summary: None,
            items: vec![Item {
                number: 21,
                stem: "What can retro-walking ease?".into(),
                options: vec![],
                answer: Answer::Text(vec![answer.into()]),
                evidence: "can ease knee pain".into(),
            }],
        }
    }

    #[test]
    fn grounded_short_answer_passes() {
        let issues = validate_task(&short_answer_task("knee pain"), Some(&passage()));
        assert!(!has_errors(&issues), "{issues:?}");
    }

    #[test]
    fn ungrounded_or_too_long_answers_fail() {
        assert!(has_errors(&validate_task(&short_answer_task("hip pain"), Some(&passage()))));
        assert!(has_errors(&validate_task(&short_answer_task("very bad knee pain"), Some(&passage()))));
    }

    #[test]
    fn tfng_requires_tfng_answers() {
        let mut task = short_answer_task("knee pain");
        task.spec = TaskSpec::new(TaskKind::TrueFalseNotGiven, 21, 21);
        assert!(has_errors(&validate_task(&task, None)));
        task.items[0].answer = Answer::Tfng(Tfng::NotGiven);
        assert!(!has_errors(&validate_task(&task, None)));
    }

    #[test]
    fn multiple_select_needs_distinct_letters() {
        let options: Vec<Choice> = "ABCDE".chars().map(|c| Choice { letter: c, text: format!("option {c}") }).collect();
        let item = |n: u8, l: char| Item {
            number: n,
            stem: format!("statement {n}"),
            options: vec![],
            answer: Answer::Letters(vec![l]),
            evidence: String::new(),
        };
        let mut task = Task {
            spec: TaskSpec::new(TaskKind::MultipleSelect { choose: 2, options: 5 }, 11, 12),
            instruction: String::new(),
            shared_options: options,
            summary: None,
            items: vec![item(11, 'B'), item(12, 'B')],
        };
        assert!(has_errors(&validate_task(&task, None)));
        task.items[1].answer = Answer::Letters(vec!['D']);
        assert!(!has_errors(&validate_task(&task, None)));
    }

    #[test]
    fn speaker_count_follows_the_part() {
        let hsg = ExamFormat::hsg_national();
        let part1 = hsg.part(1).unwrap();
        assert!(validate_speakers(part1, &part1.default_speakers).is_empty());
        assert!(has_errors(&validate_speakers(part1, &part1.default_speakers[..2])));
    }

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(normalize("Text-neck, eye strain and headaches!"), "text neck eye strain and headaches");
        assert_eq!(normalize("Samara\u{2019}s sister"), "samaras sister");
    }
}
