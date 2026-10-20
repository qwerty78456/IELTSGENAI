//! Invariants checked before anything reaches the teacher.
//!
//! Validation never mutates; it returns a list of issues so the UI can show
//! them next to the draft. `Severity::Error` means the item is unusable as a
//! key; `Severity::Warning` means a teacher should look at it.

use serde::{Deserialize, Serialize};

use super::exam::Exam;
use super::format::{PartSpec, TaskKind, WordLimit};
use super::passage::Passage;
use super::speaker::SpeakerConfig;
use super::speech::{display_text, markup_problems};
use super::task::{Answer, Choice, Item, Task};
use super::voice::voice_conflicts;

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
        Self {
            severity: Severity::Error,
            item,
            message: message.into(),
        }
    }

    fn warning(item: Option<u8>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            item,
            message: message.into(),
        }
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

/// The issue that makes a draft unusable, skipping warnings listed before it.
pub fn first_error(issues: &[ValidationIssue]) -> Option<&ValidationIssue> {
    issues.iter().find(|i| i.severity == Severity::Error)
}

/// The speaker line-up must match the part's passage kind and use unique
/// labels. Voice conflicts are only warnings here, since the script does not
/// depend on the voice; the recording refuses them (`AudioRequest::validate`).
pub fn validate_speakers(spec: &PartSpec, speakers: &[SpeakerConfig]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let expected = spec.speaker_count() as usize;
    if speakers.len() != expected {
        issues.push(ValidationIssue::error(
            None,
            format!(
                "{} needs exactly {} speaker(s), found {}",
                spec.title,
                expected,
                speakers.len()
            ),
        ));
    }
    for (i, speaker) in speakers.iter().enumerate() {
        if speakers[..i]
            .iter()
            .any(|other| other.label == speaker.label)
        {
            issues.push(ValidationIssue::error(
                None,
                format!("Duplicate speaker label \"{}\"", speaker.label),
            ));
        }
    }
    for conflict in voice_conflicts(speakers) {
        issues.push(ValidationIssue::warning(None, conflict));
    }
    issues
}

/// A passage must use only the configured labels, all of them, and fit the
/// part's duration window (by estimate). Speech markup that would be read
/// aloud, left out or lost gets a warning naming the turn.
pub fn validate_passage(
    passage: &Passage,
    spec: &PartSpec,
    speakers: &[SpeakerConfig],
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if let Some(problem) = passage.size_problem() {
        issues.push(ValidationIssue::error(None, problem));
    }
    let labels: Vec<&str> = speakers.iter().map(|s| s.label.as_str()).collect();
    let used = passage.speakers_used();
    for label in &used {
        if !labels.contains(&label.as_str()) {
            issues.push(ValidationIssue::error(
                None,
                format!("Unknown speaker label \"{label}\" in the script"),
            ));
        }
    }
    for label in &labels {
        if !used.iter().any(|u| u == label) {
            issues.push(ValidationIssue::warning(
                None,
                format!("{label} never speaks"),
            ));
        }
    }
    let minutes = passage.estimated_minutes();
    if minutes < spec.min_minutes * 0.8 {
        issues.push(ValidationIssue::warning(
            None,
            format!(
                "Script is short: about {minutes:.1} min, {} expects {}",
                spec.title,
                spec.duration_label()
            ),
        ));
    } else if minutes > spec.max_minutes * 1.25 {
        issues.push(ValidationIssue::warning(
            None,
            format!(
                "Script is long: about {minutes:.1} min, {} expects {}",
                spec.title,
                spec.duration_label()
            ),
        ));
    }
    // `[FILL IN]` is markup to `display_text`, so it is looked for in the raw text.
    if passage
        .lines
        .iter()
        .any(|l| l.display_text().contains("___") || l.text.contains("[FILL"))
    {
        issues.push(ValidationIssue::error(
            None,
            "The script contains gaps; scripts must be complete",
        ));
    }
    for (i, line) in passage.lines.iter().enumerate() {
        for problem in markup_problems(&line.text) {
            issues.push(ValidationIssue::warning(
                None,
                format!("{} (turn {}): {problem}", line.speaker, i + 1),
            ));
        }
    }
    issues.extend(line_up_drift(passage, speakers));
    issues
}

/// Warnings when the speakers are no longer the people the script was
/// written for (`Passage::written_for`): gender, accent or role changed, so
/// names, pronouns and wording may not fit. A new voice alone changes nothing
/// in the script; a script from before 0.8 has nothing to compare with.
fn line_up_drift(passage: &Passage, speakers: &[SpeakerConfig]) -> Vec<ValidationIssue> {
    if !passage.speakers_changed(speakers).script {
        return Vec::new();
    }
    let mut issues: Vec<ValidationIssue> = speakers
        .iter()
        .filter_map(|now| {
            let then = passage
                .written_for
                .iter()
                .find(|then| then.label == now.label)?;
            let changed =
                then.gender != now.gender || then.accent != now.accent || then.role != now.role;
            changed.then(|| {
                ValidationIssue::warning(
                    None,
                    format!(
                        "{} was {} when the script was written and is now {}; rewrite the script or keep it",
                        now.label,
                        then.profile(),
                        now.profile()
                    ),
                )
            })
        })
        .collect();
    if issues.is_empty() {
        issues.push(ValidationIssue::warning(
            None,
            "The script was written for other speakers; rewrite the script or keep it",
        ));
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
            format!(
                "Items must be numbered {} to {} in order, found {:?}",
                spec.first, spec.last, actual
            ),
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
    } else if !task.shared_options.is_empty() {
        issues.push(ValidationIssue::warning(
            None,
            format!(
                "{} does not use shared options; they are not printed",
                spec.kind.label()
            ),
        ));
    }
    if spec.kind.uses_summary() {
        match task.printed_summary() {
            None => issues.push(ValidationIssue::error(
                None,
                format!(
                    "{} needs its text with the gaps (n)______",
                    spec.kind.label()
                ),
            )),
            Some(text) => {
                for number in &expected {
                    let (with_blank, bare) = gap_occurrences(text, *number);
                    match with_blank + bare {
                        0 => issues.push(ValidationIssue::error(
                            Some(*number),
                            format!("Gap ({number})______ is missing from the text"),
                        )),
                        // The paper still prints a numbered answer box.
                        1 if bare == 1 => issues.push(ValidationIssue::warning(
                            Some(*number),
                            format!("Gap ({number}) has no blank line (______) after it"),
                        )),
                        1 => {}
                        times => issues.push(ValidationIssue::error(
                            Some(*number),
                            format!("Gap ({number}) appears {times} times in the text"),
                        )),
                    }
                }
            }
        }
    } else if task
        .summary
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
    {
        issues.push(ValidationIssue::warning(
            None,
            format!(
                "{} does not use a summary or notes text; it is not printed",
                spec.kind.label()
            ),
        ));
    }
    let haystack = passage.map(|p| normalize(&p.plain_text()));
    for item in &task.items {
        issues.extend(validate_item(item, task, haystack.as_deref()));
    }
    if let TaskKind::MultipleSelect { choose, .. } = spec.kind {
        let mut letters: Vec<char> = task
            .items
            .iter()
            .flat_map(|i| i.answer.letters().iter().copied())
            .collect();
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

/// Structural completeness of an exam before it is exported or recorded:
/// every part has a script, every `TaskSpec` has a task and, once nothing is
/// missing, item numbers run from 1 to the format's total without gaps or
/// repeats. Content issues stay with the drafts that produced them.
pub fn validate_exam(exam: &Exam) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut missing = false;
    for part in &exam.parts {
        if part.passage.is_none() {
            issues.push(ValidationIssue::error(
                None,
                format!("{} has no script yet", part.spec.title),
            ));
            missing = true;
        }
        for spec in part.missing_tasks() {
            issues.push(ValidationIssue::error(
                None,
                format!(
                    "{} is missing {} (questions {})",
                    part.spec.title,
                    spec.kind.label(),
                    spec.range_label()
                ),
            ));
            missing = true;
        }
    }
    if !missing {
        let total = exam.format.total_items();
        let actual: Vec<u8> = exam
            .parts
            .iter()
            .flat_map(|p| p.tasks.iter())
            .flat_map(|t| t.items.iter().map(|i| i.number))
            .collect();
        let expected: Vec<u8> = (1..=total).collect();
        if actual != expected {
            issues.push(ValidationIssue::error(
                None,
                format!("Items must be numbered 1 to {total} across the exam in order, without gaps or repeats"),
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
        TaskKind::SummaryCompletion(_)
            | TaskKind::NoteCompletion(_)
            | TaskKind::MultipleSelect { .. }
    );
    if item.stem.trim().is_empty() && !is_gap {
        issues.push(ValidationIssue::error(number, "Empty question"));
    }
    if kind.uses_summary() && task.printed_summary().is_some() && !item.stem.trim().is_empty() {
        issues.push(ValidationIssue::warning(
            number,
            format!(
                "The stem is not printed for {}; the text with the gaps is",
                kind.label()
            ),
        ));
    }
    if matches!(kind, TaskKind::SentenceCompletion(_))
        && !item.stem.trim().is_empty()
        && !item.stem.contains("___")
    {
        issues.push(ValidationIssue::error(
            number,
            "The sentence needs a blank (______) where the answer goes",
        ));
    }
    match kind {
        TaskKind::TrueFalseNotGiven => {
            if !matches!(item.answer, Answer::Tfng(_)) {
                issues.push(ValidationIssue::error(number, "Answer must be T, F or NG"));
            }
        }
        TaskKind::MultipleChoice { options } => {
            if item.options.len() != *options as usize {
                issues.push(ValidationIssue::error(
                    number,
                    format!("Needs {options} options, found {}", item.options.len()),
                ));
            }
            issues.extend(check_lettering(&item.options, number));
            issues.extend(check_letter_answer(item, &item.options, 1));
        }
        TaskKind::WhoMentioned { .. }
        | TaskKind::Matching { .. }
        | TaskKind::MultipleSelect { .. } => {
            issues.extend(check_letter_answer(item, &task.shared_options, 1));
        }
        TaskKind::ShortAnswer(limit)
        | TaskKind::SummaryCompletion(limit)
        | TaskKind::NoteCompletion(limit)
        | TaskKind::SentenceCompletion(limit) => match &item.answer {
            Answer::Text(variants) if !variants.is_empty() => {
                for variant in variants {
                    issues.extend(check_text_answer(variant, *limit, number));
                }
                if let Some(haystack) = haystack
                    && !variants.iter().any(|v| haystack.contains(&normalize(v)))
                {
                    issues.push(ValidationIssue::error(
                        number,
                        format!("Key \"{}\" does not occur in the script", variants[0]),
                    ));
                }
            }
            _ => issues.push(ValidationIssue::error(
                number,
                "Answer must be text taken from the recording",
            )),
        },
    }
    if let Some(haystack) = haystack {
        if item.evidence.trim().is_empty() {
            issues.push(ValidationIssue::warning(
                number,
                "No evidence quoted from the script",
            ));
        } else if !haystack.contains(&normalize(&item.evidence)) {
            issues.push(ValidationIssue::warning(
                number,
                "The quoted evidence is not verbatim from the script",
            ));
        }
    }
    issues
}

fn check_lettering(options: &[Choice], item: Option<u8>) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for (i, option) in options.iter().enumerate() {
        let expected = (b'A' + i as u8) as char;
        if option.letter != expected {
            issues.push(ValidationIssue::error(
                item,
                format!(
                    "Options must be lettered A, B, C...; found {}",
                    option.letter
                ),
            ));
            break;
        }
        if option.text.trim().is_empty() {
            issues.push(ValidationIssue::error(
                item,
                format!("Option {} is empty", option.letter),
            ));
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
            issues.push(ValidationIssue::error(
                None,
                format!("Option letter {} is used twice", option.letter),
            ));
        }
        if option.text.trim().is_empty() {
            issues.push(ValidationIssue::error(
                None,
                format!("Option {} is empty", option.letter),
            ));
        }
    }
    issues
}

fn check_letter_answer(
    item: &Item,
    options: &[Choice],
    expected_count: usize,
) -> Vec<ValidationIssue> {
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
                    issues.push(ValidationIssue::error(
                        Some(item.number),
                        format!("Answer {letter} is not one of the options"),
                    ));
                }
            }
        }
        _ => issues.push(ValidationIssue::error(
            Some(item.number),
            "Answer must be a letter",
        )),
    }
    issues
}

/// Speech markup removed, lower-case, punctuation stripped, apostrophes
/// removed, single spaces: the comparison form used for grounding checks, so
/// a key or quote matches whether or not a `<sigh>` sits in the script.
pub fn normalize(text: &str) -> String {
    let text = display_text(text);
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

/// The word limit of one accepted answer. With "AND/OR A NUMBER" the answer
/// may add one number to its words: "15 May", "25 dollars", "3.5 million",
/// "one hundred and twenty". Without it, every token counts and a number
/// token is refused ("25 cats"); words that hold digits ("A4", "COVID-19")
/// are words.
fn check_text_answer(variant: &str, limit: WordLimit, item: Option<u8>) -> Vec<ValidationIssue> {
    let shape = answer_shape(variant);
    if shape.tokens == 0 {
        return vec![ValidationIssue::error(item, "Empty answer")];
    }
    let mut issues = Vec::new();
    let max = usize::from(limit.max_words);
    if limit.allow_number {
        if shape.numbers > 1 {
            issues.push(ValidationIssue::error(
                item,
                format!(
                    "\"{variant}\" has {} numbers; the limit is {}",
                    shape.numbers,
                    limit.instruction()
                ),
            ));
        }
        if shape.words > max {
            let besides = if shape.numbers > 0 {
                " besides the number"
            } else {
                ""
            };
            issues.push(ValidationIssue::error(
                item,
                format!(
                    "\"{variant}\" has {} words{besides}; the limit is {}",
                    shape.words,
                    limit.instruction()
                ),
            ));
        }
    } else {
        if shape.tokens > max {
            issues.push(ValidationIssue::error(
                item,
                format!(
                    "\"{variant}\" has {} words; the limit is {}",
                    shape.tokens,
                    limit.instruction()
                ),
            ));
        }
        if shape.has_number_token {
            issues.push(ValidationIssue::error(
                item,
                format!("\"{variant}\" contains a number but numbers are not allowed"),
            ));
        }
    }
    issues
}

/// How an answer counts: whitespace tokens, the words outside numbers, the
/// numbers, and whether one token is written in digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AnswerShape {
    tokens: usize,
    words: usize,
    numbers: usize,
    has_number_token: bool,
}

/// A number is a run of tokens: digits ("15", "$25", "2,500", "15th",
/// "10am"), optionally followed by scale words ("3.5 million"), or cardinal
/// words ("twenty five", "one hundred and twenty"; "and" only between them).
/// Ordinals, "half" or "quarter" are words: "first aid" is two words.
fn answer_shape(text: &str) -> AnswerShape {
    #[derive(PartialEq)]
    enum Run {
        None,
        Digits,
        Words,
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut shape = AnswerShape {
        tokens: tokens.len(),
        words: 0,
        numbers: 0,
        has_number_token: false,
    };
    let mut run = Run::None;
    for (i, token) in tokens.iter().enumerate() {
        let word = word_form(token);
        if is_digit_number_token(token) {
            shape.has_number_token = true;
            shape.numbers += 1;
            run = Run::Digits;
        } else if is_scale_word(&word) && run != Run::None {
            run = Run::Words;
        } else if is_cardinal_word(&word) {
            if run != Run::Words {
                shape.numbers += 1;
            }
            run = Run::Words;
        } else if word == "and"
            && run == Run::Words
            && tokens
                .get(i + 1)
                .is_some_and(|next| is_cardinal_word(&word_form(next)))
        {
            // "one hundred and twenty": still the same number.
        } else {
            shape.words += 1;
            run = Run::None;
        }
    }
    shape
}

/// Lower case, without the punctuation around a word ("Twenty-five," -> "twenty-five").
fn word_form(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
        .to_lowercase()
}

/// A token written in digits: "15", "$25", "2,500", "3.5", "10:30", "24/7",
/// "50%", "15th", "10am". "A4", "M25", "COVID-19" and "24-hour" are words.
fn is_digit_number_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| {
        matches!(
            c,
            '$' | '£' | '€' | '¥' | '%' | ':' | '.' | ',' | ';' | '!' | '?' | '(' | ')' | '"'
        )
    });
    let lower = trimmed.to_lowercase();
    let core = ["st", "nd", "rd", "th", "am", "pm"]
        .iter()
        .find_map(|suffix| lower.strip_suffix(*suffix))
        .unwrap_or(lower.as_str());
    core.chars().any(|c| c.is_ascii_digit())
        && core
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ':' | '/' | '-'))
}

/// "zero" to "nineteen", the tens, the scale words, and hyphenated
/// combinations of them ("twenty-five").
fn is_cardinal_word(word: &str) -> bool {
    const CARDINALS: [&str; 28] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
        "thirty",
        "forty",
        "fifty",
        "sixty",
        "seventy",
        "eighty",
        "ninety",
    ];
    !word.is_empty()
        && word
            .split('-')
            .all(|part| CARDINALS.contains(&part) || is_scale_word(part))
}

fn is_scale_word(word: &str) -> bool {
    matches!(word, "hundred" | "thousand" | "million" | "billion")
}

/// How often the marker "(n)" appears in a summary or note text: followed
/// (after spaces) by a blank "___", and without one. "(1)" inside "(11)" is
/// not a match.
fn gap_occurrences(text: &str, number: u8) -> (usize, usize) {
    let marker = format!("({number})");
    let (mut with_blank, mut bare) = (0, 0);
    for (at, _) in text.match_indices(&marker) {
        let rest = text[at + marker.len()..].trim_start_matches([' ', '\t']);
        if rest.starts_with("___") {
            with_blank += 1;
        } else {
            bare += 1;
        }
    }
    (with_blank, bare)
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
        assert!(has_errors(&validate_task(
            &short_answer_task("hip pain"),
            Some(&passage())
        )));
        assert!(has_errors(&validate_task(
            &short_answer_task("very bad knee pain"),
            Some(&passage())
        )));
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
        let options: Vec<Choice> = "ABCDE"
            .chars()
            .map(|c| Choice {
                letter: c,
                text: format!("option {c}"),
            })
            .collect();
        let item = |n: u8, l: char| Item {
            number: n,
            stem: format!("statement {n}"),
            options: vec![],
            answer: Answer::Letters(vec![l]),
            evidence: String::new(),
        };
        let mut task = Task {
            spec: TaskSpec::new(
                TaskKind::MultipleSelect {
                    choose: 2,
                    options: 5,
                },
                11,
                12,
            ),
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
        assert!(has_errors(&validate_speakers(
            part1,
            &part1.default_speakers[..2]
        )));
    }

    #[test]
    fn shared_voice_warns() {
        use crate::domain::voice::{Voice, VoiceSource};
        let hsg = ExamFormat::hsg_national();
        let part1 = hsg.part(1).unwrap();
        let zephyr = Voice {
            id: "Zephyr".into(),
            name: "Zephyr".into(),
            gender: crate::domain::Gender::Female,
            accent: crate::domain::Accent::British,
            source: VoiceSource::Library,
            description: String::new(),
        };
        let mut speakers = part1.default_speakers.clone();
        for speaker in &mut speakers {
            *speaker = speaker.clone().with_voice(zephyr.clone());
        }
        let issues = validate_speakers(part1, &speakers);
        assert!(!has_errors(&issues), "{issues:?}");
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].message.contains("share the voice Zephyr"));
    }

    #[test]
    fn script_written_for_other_speakers_warns() {
        use crate::domain::speaker::{Gender, SpeakerRole};
        use crate::domain::voice::{Voice, VoiceSource};
        let hsg = ExamFormat::hsg_national();
        let part1 = hsg.part(1).unwrap();
        let line_up = part1.default_speakers.clone();
        let labels: Vec<String> = line_up.iter().map(|s| s.label.clone()).collect();
        let script: Vec<String> = labels
            .iter()
            .map(|l| format!("{l}: {}", "word ".repeat(150)))
            .collect();
        let parsed = Passage::parse(1, "topic", &script.join("\n"), &labels).unwrap();
        let passage = parsed.clone().for_speakers(&line_up);
        let baseline = validate_passage(&passage, part1, &line_up);
        assert!(
            baseline.iter().all(|i| !i.message.contains("rewrite")),
            "{baseline:?}"
        );

        // A new voice alone: the script still fits.
        let mut voiced = line_up.clone();
        voiced[1] = voiced[1].clone().with_voice(Voice {
            id: "en-gb-b".into(),
            name: "B".into(),
            gender: voiced[1].gender,
            accent: voiced[1].accent,
            source: VoiceSource::Library,
            description: String::new(),
        });
        assert_eq!(validate_passage(&passage, part1, &voiced), baseline);

        // Another role and gender for Speaker B: one warning naming both line-ups.
        let mut edited = line_up.clone();
        edited[1].role = SpeakerRole::Professor;
        edited[1].gender = match edited[1].gender {
            Gender::Male => Gender::Female,
            Gender::Female => Gender::Male,
        };
        let issues = validate_passage(&passage, part1, &edited);
        assert!(!has_errors(&issues), "{issues:?}");
        let drift: Vec<&ValidationIssue> = issues
            .iter()
            .filter(|i| i.message.contains("when the script was written"))
            .collect();
        assert_eq!(drift.len(), 1, "{issues:?}");
        assert_eq!(drift[0].severity, Severity::Warning);
        assert_eq!(
            drift[0].message,
            format!(
                "Speaker B was {} when the script was written and is now {}; rewrite the script or keep it",
                line_up[1].profile(),
                edited[1].profile()
            )
        );

        // Kept for the new line-up, or written before 0.8: no warning.
        let kept = passage.clone().for_speakers(&edited);
        assert_eq!(
            validate_passage(&kept, part1, &edited).len(),
            baseline.len()
        );
        assert_eq!(
            validate_passage(&parsed, part1, &edited).len(),
            baseline.len()
        );
    }

    #[test]
    fn grounding_ignores_speech_markup() {
        let passage = Passage::parse(
            3,
            "retro-walking",
            "Speaker A: Walking backwards can ease <sigh> knee pain |mhm| and improve [music] balance.",
            &["Speaker A".to_string()],
        )
        .unwrap();
        // Evidence quoted without the tag, and with it.
        for evidence in [
            "can ease knee pain",
            "can ease <sigh> knee pain and improve",
        ] {
            let mut task = short_answer_task("knee pain");
            task.items[0].evidence = evidence.into();
            let issues = validate_task(&task, Some(&passage));
            assert!(issues.is_empty(), "{evidence}: {issues:?}");
        }
        // A key that only exists across the removed note still matches.
        let issues = validate_task(&short_answer_task("improve balance"), Some(&passage));
        assert!(!has_errors(&issues), "{issues:?}");
        assert_eq!(normalize("Well <sigh>, it's |mhm| FINE."), "well its fine");
    }

    #[test]
    fn markup_problems_are_warnings_naming_the_turn() {
        let hsg = ExamFormat::hsg_national();
        let part1 = hsg.part(1).unwrap();
        let speakers = part1.default_speakers.clone();
        let labels: Vec<String> = speakers.iter().map(|s| s.label.clone()).collect();
        let filler = "word ".repeat(150);
        let script = format!(
            "{}: {filler}
{}: {filler} and <smirk> then
{}: {filler} [FILL IN]",
            labels[0], labels[1], labels[2]
        );
        let passage = Passage::parse(1, "topic", &script, &labels).unwrap();
        let issues = validate_passage(&passage, part1, &speakers);
        let markup: Vec<&ValidationIssue> = issues
            .iter()
            .filter(|i| i.message.contains("(turn "))
            .collect();
        assert_eq!(markup.len(), 1, "{issues:?}");
        assert_eq!(markup[0].severity, Severity::Warning);
        assert_eq!(
            markup[0].message,
            format!(
                "{} (turn 2): <smirk> is not a speech tag; it is left out of the recording",
                labels[1]
            )
        );
        // The gap is still an error, reported once.
        assert!(has_errors(&issues));
        assert_eq!(
            issues.iter().filter(|i| i.message.contains("gaps")).count(),
            1
        );
    }

    #[test]
    fn first_error_skips_warnings() {
        let issues = [
            ValidationIssue::warning(None, "look at this"),
            ValidationIssue::error(None, "unusable"),
        ];
        assert_eq!(first_error(&issues).unwrap().message, "unusable");
        assert_eq!(first_error(&issues[..1]), None);
    }

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(
            normalize("Text-neck, eye strain and headaches!"),
            "text neck eye strain and headaches"
        );
        assert_eq!(normalize("Samara\u{2019}s sister"), "samaras sister");
    }

    fn stub_task(spec: TaskSpec) -> Task {
        let items = (spec.first..=spec.last)
            .map(|n| Item {
                number: n,
                stem: format!("statement {n}"),
                options: vec![],
                answer: Answer::Tfng(Tfng::True),
                evidence: String::new(),
            })
            .collect();
        Task {
            spec,
            instruction: String::new(),
            shared_options: vec![],
            summary: None,
            items,
        }
    }

    /// Every part scripted and every task present; the content is placeholder.
    fn filled_exam() -> Exam {
        let mut exam = Exam::new(ExamFormat::hsg_national(), "Mock 1", "news");
        for part in &mut exam.parts {
            let labels: Vec<String> = part.speakers.iter().map(|s| s.label.clone()).collect();
            let script: Vec<String> = labels
                .iter()
                .map(|l| format!("{l}: Hello and welcome."))
                .collect();
            part.passage = Some(
                Passage::parse(part.spec.number, "topic", &script.join("\n"), &labels).unwrap(),
            );
            part.tasks = part.spec.tasks.iter().cloned().map(stub_task).collect();
        }
        exam
    }

    #[test]
    fn empty_exam_lists_every_missing_script_and_task() {
        let issues = validate_exam(&Exam::new(ExamFormat::hsg_national(), "Mock 1", "news"));
        assert_eq!(
            issues
                .iter()
                .filter(|i| i.message.contains("no script yet"))
                .count(),
            4
        );
        assert_eq!(issues.len(), 4 + 7, "{issues:?}");
        assert!(has_errors(&issues));
    }

    #[test]
    fn complete_exam_has_no_structural_issues() {
        let issues = validate_exam(&filled_exam());
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn missing_task_is_named_by_range() {
        let mut exam = filled_exam();
        exam.parts[1].tasks.remove(1);
        let issues = validate_exam(&exam);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(
            issues[0].message.contains("13 - 15"),
            "{}",
            issues[0].message
        );
    }

    /// A one-item short-answer task with this limit and key, and its error messages.
    fn answer_errors(limit: WordLimit, answer: &str) -> Vec<String> {
        let mut task = short_answer_task(answer);
        task.spec.kind = TaskKind::ShortAnswer(limit);
        validate_task(&task, None)
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| i.message)
            .collect()
    }

    #[test]
    fn words_or_number_allows_one_word_plus_a_number() {
        let limit = WordLimit::words_or_number(1);
        for fine in [
            "15",
            "May",
            "15 May",
            "May 15",
            "15th May",
            "25 dollars",
            "$25",
            "3.5 million",
            "10 am",
            "10am",
            "2,500 pounds",
            "twenty five",
            "twenty-five",
            "one hundred and twenty",
            "24/7",
        ] {
            assert_eq!(answer_errors(limit, fine), Vec::<String>::new(), "{fine}");
        }
        for too_long in ["big red bus", "first aid", "half price", "second floor"] {
            let errors = answer_errors(limit, too_long);
            assert_eq!(errors.len(), 1, "{too_long}: {errors:?}");
            assert!(errors[0].contains("words; the limit is"), "{errors:?}");
        }
        let errors = answer_errors(limit, "15 May 2026");
        assert!(errors[0].contains("has 2 numbers"), "{errors:?}");
        let errors = answer_errors(limit, "the 15 May");
        assert_eq!(
            errors,
            [
                "\"the 15 May\" has 2 words besides the number; the limit is NO MORE THAN ONE WORD AND/OR A NUMBER"
            ]
        );
    }

    #[test]
    fn and_counts_only_between_number_words() {
        let limit = WordLimit::words_or_number(2);
        for too_long in ["salt and pepper", "fish and chips"] {
            assert!(!answer_errors(limit, too_long).is_empty(), "{too_long}");
        }
        for fine in ["two hundred and fifty grams", "bread rolls", "fifty grams"] {
            assert!(answer_errors(limit, fine).is_empty(), "{fine}");
        }
    }

    #[test]
    fn words_without_number_reject_only_number_tokens() {
        let limit = WordLimit::words(2);
        for number in ["25 cats", "25", "$50"] {
            let errors = answer_errors(limit, number);
            assert!(
                errors.iter().any(|e| e.contains("numbers are not allowed")),
                "{number}: {errors:?}"
            );
        }
        for word in [
            "A4 paper",
            "M25",
            "COVID-19",
            "3D printer",
            "24-hour service",
            "twenty cats",
        ] {
            assert!(answer_errors(limit, word).is_empty(), "{word}");
        }
        assert!(!answer_errors(WordLimit::words(2), "very bad knee pain").is_empty());
    }

    /// Already true in 0.10.0 (words are split on whitespace only); kept as a guard.
    #[test]
    fn hyphenated_compound_counts_as_one_word() {
        for word in ["check-in", "part-time", "up-to-date"] {
            assert!(
                answer_errors(WordLimit::words(1), word).is_empty(),
                "{word}"
            );
        }
        assert!(!answer_errors(WordLimit::words(1), "check in desk").is_empty());
    }

    #[test]
    fn spelled_out_variant_is_one_number() {
        let passage = Passage::parse(
            1,
            "birds",
            "Speaker A: There are one hundred birds in the sanctuary.",
            &["Speaker A".to_string()],
        )
        .unwrap();
        let mut task = short_answer_task("100");
        task.spec.kind = TaskKind::ShortAnswer(WordLimit::words_or_number(1));
        task.items[0].answer = Answer::Text(vec!["100".into(), "one hundred".into()]);
        task.items[0].evidence = "one hundred birds".into();
        let issues = validate_task(&task, Some(&passage));
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// A gap task over items 1-2 with this text and these stems.
    fn gap_task(kind: TaskKind, summary: Option<&str>, stem: &str) -> Task {
        let item = |number| Item {
            number,
            stem: stem.into(),
            options: vec![],
            answer: Answer::Text(vec!["library".into()]),
            evidence: String::new(),
        };
        Task {
            spec: TaskSpec::new(kind, 1, 2),
            instruction: "Complete the notes below.".into(),
            shared_options: vec![],
            summary: summary.map(String::from),
            items: vec![item(1), item(2)],
        }
    }

    fn messages(issues: &[ValidationIssue], severity: Severity) -> Vec<String> {
        issues
            .iter()
            .filter(|i| i.severity == severity)
            .map(|i| i.message.clone())
            .collect()
    }

    #[test]
    fn note_and_summary_completion_need_their_text_and_gaps() {
        let limit = WordLimit::words_or_number(1);
        for kind in [
            TaskKind::NoteCompletion(limit),
            TaskKind::SummaryCompletion(limit),
        ] {
            let label = kind.label();
            for missing in [None, Some(""), Some("   ")] {
                let issues = validate_task(&gap_task(kind.clone(), missing, ""), None);
                assert_eq!(
                    messages(&issues, Severity::Error),
                    [format!("{label} needs its text with the gaps (n)______")],
                    "{missing:?}"
                );
            }
            let check = |text: &str| validate_task(&gap_task(kind.clone(), Some(text), ""), None);
            let issues = check("Meet at (1)______ on Monday.");
            assert_eq!(
                messages(&issues, Severity::Error),
                ["Gap (2)______ is missing from the text"]
            );
            let issues = check("Meet at (1) ______ on (2).");
            assert!(!has_errors(&issues), "{issues:?}");
            assert_eq!(
                messages(&issues, Severity::Warning),
                ["Gap (2) has no blank line (______) after it"]
            );
            let issues = check("(1)______ and (2)______, as in (1)______.");
            assert_eq!(
                messages(&issues, Severity::Error),
                ["Gap (1) appears 2 times in the text"]
            );
            // "(1)" is not found inside "(11)".
            assert!(check("(1)______ (2)______ (11)______").is_empty());
        }
    }

    #[test]
    fn unused_shared_options_and_summary_warn() {
        let mut task = short_answer_task("knee pain");
        task.shared_options = vec![Choice {
            letter: 'A',
            text: "stray".into(),
        }];
        task.summary = Some("A stray summary.".into());
        let issues = validate_task(&task, Some(&passage()));
        assert!(!has_errors(&issues), "{issues:?}");
        assert_eq!(
            messages(&issues, Severity::Warning),
            [
                "Short answer does not use shared options; they are not printed",
                "Short answer does not use a summary or notes text; it is not printed"
            ]
        );
        task.summary = Some("  ".into());
        task.shared_options.clear();
        assert!(validate_task(&task, Some(&passage())).is_empty());
    }

    #[test]
    fn gap_task_stems_warn_only_when_the_text_hides_them() {
        let kind = TaskKind::NoteCompletion(WordLimit::words_or_number(1));
        let issues = validate_task(
            &gap_task(kind.clone(), Some("(1)______ (2)______"), "Name:"),
            None,
        );
        assert_eq!(
            messages(&issues, Severity::Warning),
            ["The stem is not printed for Note / form completion; the text with the gaps is"; 2]
        );
        // Without the text the stems are printed, so only the missing text is reported.
        let issues = validate_task(&gap_task(kind, None, "Name:"), None);
        assert!(
            messages(&issues, Severity::Warning).is_empty(),
            "{issues:?}"
        );
    }

    #[test]
    fn sentence_completion_needs_a_blank() {
        let mut task = short_answer_task("knee pain");
        task.spec.kind = TaskKind::SentenceCompletion(WordLimit::words(2));
        task.items[0].stem = "Retro-walking can ease".into();
        let issues = validate_task(&task, Some(&passage()));
        assert_eq!(
            messages(&issues, Severity::Error),
            ["The sentence needs a blank (______) where the answer goes"]
        );
        task.items[0].stem = "Retro-walking can ease ______.".into();
        assert!(!has_errors(&validate_task(&task, Some(&passage()))));
    }

    #[test]
    fn an_oversized_script_is_an_error_on_the_page() {
        let hsg = ExamFormat::hsg_national();
        let part3 = hsg.part(3).unwrap();
        let labels = vec!["Speaker A".to_string()];
        let huge = format!("Speaker A: {}", "x".repeat(40_000));
        let passage = Passage::parse(3, "topic", &huge, &labels).unwrap();
        let issues = validate_passage(&passage, part3, &part3.default_speakers);
        assert!(
            messages(&issues, Severity::Error)
                .iter()
                .any(|m| m.contains("at most 30000 characters")),
            "{issues:?}"
        );
    }

    #[test]
    fn misnumbered_items_break_contiguity() {
        let mut exam = filled_exam();
        exam.parts[2].tasks[0].items[1].number = 21;
        let issues = validate_exam(&exam);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(
            issues[0].message.contains("1 to 35"),
            "{}",
            issues[0].message
        );
    }
}
