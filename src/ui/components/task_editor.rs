//! The question editor: one question block (`Task`) in a dialog, with the
//! fields its `TaskKind` prints. The teacher's text is kept as typed and
//! turned into a `Task` only on Save; the domain validator then reports what
//! is wrong, the editor never refuses to save.

use dioxus::prelude::*;

use crate::domain::{Answer, Choice, Item, Task, TaskKind, TaskSpec, Tfng};

/// How the key of one item is chosen, by task kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerDraft {
    /// True / False / Not Given; `None` until chosen.
    Tfng(Option<Tfng>),
    /// One letter (multiple choice, matching, who-mentioned, one item of a
    /// multiple selection); `None` until chosen.
    Letter(Option<char>),
    /// Accepted answers, one per line; the first is the key.
    Text(String),
}

/// One item as the teacher edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDraft {
    pub stem: String,
    pub options: Vec<Choice>,
    pub answer: AnswerDraft,
    pub evidence: String,
}

/// A question block as the teacher edits it; `to_task` turns it back into a
/// `Task` with the original spec and item numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDraft {
    original: Task,
    initial_answers: Vec<AnswerDraft>,
    pub instruction: String,
    /// The text with the gaps, for a kind that uses one.
    pub summary: String,
    /// A summary on a kind that prints none; the teacher may remove it.
    pub stray_summary: Option<String>,
    pub shared_options: Vec<Choice>,
    pub items: Vec<ItemDraft>,
}

/// The accepted answers in a text box: one per line, blank lines ignored.
pub fn answer_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

fn letter(index: usize) -> char {
    (b'A' + (index % 26) as u8) as char
}

/// The draft of a key for `kind`, whatever variant the model sent.
fn answer_draft(kind: &TaskKind, answer: &Answer) -> AnswerDraft {
    match kind {
        TaskKind::TrueFalseNotGiven => AnswerDraft::Tfng(match answer {
            Answer::Tfng(value) => Some(*value),
            _ => None,
        }),
        TaskKind::MultipleChoice { .. }
        | TaskKind::Matching { .. }
        | TaskKind::WhoMentioned { .. }
        | TaskKind::MultipleSelect { .. } => AnswerDraft::Letter(answer.letters().first().copied()),
        TaskKind::ShortAnswer(_)
        | TaskKind::SummaryCompletion(_)
        | TaskKind::NoteCompletion(_)
        | TaskKind::SentenceCompletion(_) => AnswerDraft::Text(match answer {
            Answer::Text(variants) => variants.join("\n"),
            _ => String::new(),
        }),
    }
}

/// The key the draft stands for. An untouched or unchosen key stays as it
/// was, so the validator still reports what was wrong with it.
fn answer_of(draft: &AnswerDraft, initial: &AnswerDraft, original: &Answer) -> Answer {
    if draft == initial {
        return original.clone();
    }
    match draft {
        AnswerDraft::Tfng(Some(value)) => Answer::Tfng(*value),
        AnswerDraft::Letter(Some(letter)) => Answer::Letters(vec![*letter]),
        AnswerDraft::Text(text) => Answer::Text(answer_lines(text)),
        AnswerDraft::Tfng(None) | AnswerDraft::Letter(None) => original.clone(),
    }
}

impl TaskDraft {
    pub fn from_task(task: &Task) -> Self {
        let kind = &task.spec.kind;
        let items: Vec<ItemDraft> = task
            .items
            .iter()
            .map(|item| ItemDraft {
                stem: item.stem.clone(),
                options: item.options.clone(),
                answer: answer_draft(kind, &item.answer),
                evidence: item.evidence.clone(),
            })
            .collect();
        Self {
            original: task.clone(),
            initial_answers: items.iter().map(|i| i.answer.clone()).collect(),
            instruction: task.instruction.clone(),
            summary: if kind.uses_summary() {
                task.summary.clone().unwrap_or_default()
            } else {
                String::new()
            },
            stray_summary: if kind.uses_summary() {
                None
            } else {
                task.summary.clone()
            },
            shared_options: task.shared_options.clone(),
            items,
        }
    }

    pub fn spec(&self) -> &TaskSpec {
        &self.original.spec
    }

    pub fn number(&self, item: usize) -> u8 {
        self.original.items.get(item).map_or(0, |i| i.number)
    }

    /// The letters an item's key may be.
    pub fn letter_choices(&self, item: usize) -> Vec<char> {
        match self.spec().kind {
            TaskKind::MultipleChoice { .. } => self
                .items
                .get(item)
                .map(|i| i.options.iter().map(|o| o.letter).collect())
                .unwrap_or_default(),
            TaskKind::Matching { .. }
            | TaskKind::WhoMentioned { .. }
            | TaskKind::MultipleSelect { .. } => {
                self.shared_options.iter().map(|o| o.letter).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Adds an option to the shared list, or to `item`'s own options.
    pub fn add_option(&mut self, item: Option<usize>) {
        let initials = matches!(self.spec().kind, TaskKind::WhoMentioned { .. });
        let list = match item {
            Some(i) => match self.items.get_mut(i) {
                Some(item) => &mut item.options,
                None => return,
            },
            None => &mut self.shared_options,
        };
        let next = if initials {
            ('A'..='Z')
                .find(|c| !list.iter().any(|o| o.letter == *c))
                .unwrap_or('Z')
        } else {
            letter(list.len())
        };
        list.push(Choice {
            letter: next,
            text: String::new(),
        });
    }

    /// Removes the last option of the shared list, or of `item`'s options.
    pub fn remove_last_option(&mut self, item: Option<usize>) {
        match item {
            Some(i) => {
                if let Some(item) = self.items.get_mut(i) {
                    item.options.pop();
                }
            }
            None => {
                self.shared_options.pop();
            }
        }
    }

    pub fn to_task(&self) -> Task {
        let kind = &self.original.spec.kind;
        let summary = if kind.uses_summary() {
            (!self.summary.trim().is_empty()).then(|| self.summary.clone())
        } else {
            self.stray_summary.clone()
        };
        let items = self
            .original
            .items
            .iter()
            .zip(&self.items)
            .zip(&self.initial_answers)
            .map(|((original, draft), initial)| Item {
                number: original.number,
                stem: draft.stem.clone(),
                options: draft.options.clone(),
                answer: answer_of(&draft.answer, initial, &original.answer),
                evidence: draft.evidence.clone(),
            })
            .collect();
        Task {
            spec: self.original.spec.clone(),
            instruction: self.instruction.clone(),
            shared_options: self.shared_options.clone(),
            summary,
            items,
        }
    }
}

/// Edits one question block. Save hands the edited `Task` to `onsave`, which
/// re-validates it; Cancel, the close button and Escape drop the edits.
/// Give it a `key` per block so it starts again from each block's content.
#[component]
pub fn TaskEditModal(task: Task, onclose: EventHandler<()>, onsave: EventHandler<Task>) -> Element {
    let mut draft = use_signal(|| TaskDraft::from_task(&task));
    let current = draft();
    let kind = current.spec().kind.clone();
    let range = current.spec().range_label();
    let shares_options = kind.has_shared_options();
    let positional_letters = !matches!(kind, TaskKind::WhoMentioned { .. });
    let shows_stem = |item: &ItemDraft| match kind {
        TaskKind::MultipleSelect { .. } => !item.stem.trim().is_empty(),
        ref k if k.uses_summary() => !item.stem.trim().is_empty(),
        _ => true,
    };

    rsx! {
        div {
            class: "modal-overlay",
            onkeydown: move |evt| {
                if evt.key() == Key::Escape {
                    onclose.call(());
                }
            },
            div {
                class: "modal-content wide",
                tabindex: "-1",
                onmounted: move |evt| async move {
                    let _ = evt.set_focus(true).await;
                },
                div { class: "modal-header",
                    h3 { "Edit questions {range}: {kind.label()}" }
                    button {
                        class: "modal-close",
                        "aria-label": "Close without saving",
                        onclick: move |_| onclose.call(()),
                        "\u{00d7}"
                    }
                }

                div { class: "modal-body",
                    div { class: "form-group",
                        label { "Instruction printed above the questions" }
                        textarea {
                            class: "input-textarea editor-short",
                            value: "{current.instruction}",
                            oninput: move |evt| draft.write().instruction = evt.value(),
                        }
                    }

                    if kind.uses_summary() {
                        div { class: "form-group",
                            label { "Text with the gaps, written (1)______, (2)______ ..." }
                            textarea {
                                class: "input-textarea editor-long",
                                value: "{current.summary}",
                                oninput: move |evt| draft.write().summary = evt.value(),
                            }
                        }
                    }
                    if current.stray_summary.is_some() {
                        div { class: "note-box editor-note",
                            span { "This question type prints no summary text, but the block has some." }
                            button {
                                class: "edit-task-button",
                                onclick: move |_| draft.write().stray_summary = None,
                                "Remove the unused text"
                            }
                        }
                    }

                    if shares_options {
                        div { class: "form-group",
                            label { "Options listed above the questions" }
                            for (index, choice) in current.shared_options.iter().enumerate() {
                                div { class: "choice-row", key: "shared-{index}",
                                    if positional_letters {
                                        span { class: "choice-letter", "{choice.letter}" }
                                    } else {
                                        input {
                                            class: "form-input choice-letter-input",
                                            "aria-label": "Letter",
                                            maxlength: "1",
                                            value: "{choice.letter}",
                                            oninput: move |evt| {
                                                if let Some(c) = evt.value().chars().next()
                                                    && let Some(option) = draft.write().shared_options.get_mut(index)
                                                {
                                                    option.letter = c.to_ascii_uppercase();
                                                }
                                            },
                                        }
                                    }
                                    input {
                                        class: "form-input",
                                        value: "{choice.text}",
                                        oninput: move |evt| {
                                            if let Some(option) = draft.write().shared_options.get_mut(index) {
                                                option.text = evt.value();
                                            }
                                        },
                                    }
                                }
                            }
                            div { class: "editor-actions",
                                button {
                                    class: "edit-task-button",
                                    onclick: move |_| draft.write().add_option(None),
                                    "Add option"
                                }
                                button {
                                    class: "edit-task-button",
                                    disabled: current.shared_options.is_empty(),
                                    onclick: move |_| draft.write().remove_last_option(None),
                                    "Remove last option"
                                }
                            }
                        }
                    } else if !current.shared_options.is_empty() {
                        div { class: "note-box editor-note",
                            span { "This question type prints no shared options, but the block has some." }
                            button {
                                class: "edit-task-button",
                                onclick: move |_| draft.write().shared_options.clear(),
                                "Remove them"
                            }
                        }
                    }

                    for (index, item) in current.items.iter().enumerate() {
                        div { class: "task-item-card", key: "item-{index}",
                            h4 { "Question {current.number(index)}" }

                            if shows_stem(item) {
                                div { class: "form-group",
                                    label {
                                        if kind.uses_summary() || matches!(kind, TaskKind::MultipleSelect { .. }) {
                                            "Stem (not printed for this question type; clear it)"
                                        } else {
                                            "Question"
                                        }
                                    }
                                    input {
                                        class: "form-input",
                                        value: "{item.stem}",
                                        oninput: move |evt| {
                                            if let Some(item) = draft.write().items.get_mut(index) {
                                                item.stem = evt.value();
                                            }
                                        },
                                    }
                                }
                            }

                            if matches!(kind, TaskKind::MultipleChoice { .. }) {
                                div { class: "form-group",
                                    label { "Options" }
                                    for (o, choice) in item.options.iter().enumerate() {
                                        div { class: "choice-row", key: "option-{index}-{o}",
                                            span { class: "choice-letter", "{choice.letter}" }
                                            input {
                                                class: "form-input",
                                                value: "{choice.text}",
                                                oninput: move |evt| {
                                                    if let Some(option) = draft
                                                        .write()
                                                        .items
                                                        .get_mut(index)
                                                        .and_then(|item| item.options.get_mut(o))
                                                    {
                                                        option.text = evt.value();
                                                    }
                                                },
                                            }
                                        }
                                    }
                                    div { class: "editor-actions",
                                        button {
                                            class: "edit-task-button",
                                            onclick: move |_| draft.write().add_option(Some(index)),
                                            "Add option"
                                        }
                                        button {
                                            class: "edit-task-button",
                                            disabled: item.options.is_empty(),
                                            onclick: move |_| draft.write().remove_last_option(Some(index)),
                                            "Remove last option"
                                        }
                                    }
                                }
                            }

                            div { class: "form-group",
                                {match item.answer.clone() {
                                    AnswerDraft::Tfng(chosen) => rsx! {
                                        label { "Key" }
                                        select {
                                            class: "form-select",
                                            onchange: move |evt| {
                                                let value = match evt.value().as_str() {
                                                    "T" => Some(Tfng::True),
                                                    "F" => Some(Tfng::False),
                                                    "NG" => Some(Tfng::NotGiven),
                                                    _ => None,
                                                };
                                                if let Some(item) = draft.write().items.get_mut(index) {
                                                    item.answer = AnswerDraft::Tfng(value);
                                                }
                                            },
                                            option { value: "", selected: chosen.is_none(), "Choose..." }
                                            option { value: "T", selected: chosen == Some(Tfng::True), "True (T)" }
                                            option { value: "F", selected: chosen == Some(Tfng::False), "False (F)" }
                                            option { value: "NG", selected: chosen == Some(Tfng::NotGiven), "Not Given (NG)" }
                                        }
                                    },
                                    AnswerDraft::Letter(chosen) => {
                                        let letters = current.letter_choices(index);
                                        let unknown = chosen.filter(|c| !letters.contains(c));
                                        rsx! {
                                            label { "Key" }
                                            select {
                                                class: "form-select",
                                                onchange: move |evt| {
                                                    let value = evt.value().chars().next();
                                                    if let Some(item) = draft.write().items.get_mut(index) {
                                                        item.answer = AnswerDraft::Letter(value);
                                                    }
                                                },
                                                option { value: "", selected: chosen.is_none(), "Choose..." }
                                                for c in letters {
                                                    option { key: "{c}", value: "{c}", selected: chosen == Some(c), "{c}" }
                                                }
                                                if let Some(c) = unknown {
                                                    option { value: "{c}", selected: true, "{c} (not an option)" }
                                                }
                                            }
                                        }
                                    }
                                    AnswerDraft::Text(text) => rsx! {
                                        label { "Accepted answers, one per line; the first is the key" }
                                        textarea {
                                            class: "input-textarea editor-short",
                                            value: "{text}",
                                            oninput: move |evt| {
                                                if let Some(item) = draft.write().items.get_mut(index) {
                                                    item.answer = AnswerDraft::Text(evt.value());
                                                }
                                            },
                                        }
                                    },
                                }}
                            }

                            div { class: "form-group",
                                label { "Evidence: the words of the script that give the key" }
                                input {
                                    class: "form-input",
                                    value: "{item.evidence}",
                                    oninput: move |evt| {
                                        if let Some(item) = draft.write().items.get_mut(index) {
                                            item.evidence = evt.value();
                                        }
                                    },
                                }
                            }
                        }
                    }
                }

                div { class: "modal-footer",
                    button {
                        class: "button-secondary",
                        onclick: move |_| onclose.call(()),
                        "Cancel"
                    }
                    button {
                        class: "button-primary",
                        onclick: move |_| onsave.call(draft.peek().to_task()),
                        "Save changes"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WordLimit;

    fn item(number: u8, answer: Answer) -> Item {
        Item {
            number,
            stem: format!("Question {number}"),
            options: vec![],
            answer,
            evidence: String::new(),
        }
    }

    fn task(kind: TaskKind, answers: Vec<Answer>) -> Task {
        let first = 21;
        Task {
            spec: TaskSpec::new(kind, first, first + answers.len() as u8 - 1),
            instruction: "Answer.".into(),
            shared_options: vec![],
            summary: None,
            items: answers
                .into_iter()
                .enumerate()
                .map(|(i, a)| item(first + i as u8, a))
                .collect(),
        }
    }

    #[test]
    fn untouched_drafts_give_back_the_same_task() {
        let original = task(
            TaskKind::ShortAnswer(WordLimit::words(2)),
            vec![Answer::Text(vec!["knee pain".into(), "pain".into()])],
        );
        assert_eq!(TaskDraft::from_task(&original).to_task(), original);
    }

    #[test]
    fn text_answers_keep_spaces_slashes_and_commas() {
        let original = task(
            TaskKind::ShortAnswer(WordLimit::words_or_number(2)),
            vec![Answer::Text(vec!["post".into()])],
        );
        let mut draft = TaskDraft::from_task(&original);
        draft.items[0].answer =
            AnswerDraft::Text("dynamic retention x\n2,500 pounds\n\n  24/7 \nSmith, John".into());
        let edited = draft.to_task();
        assert_eq!(
            edited.items[0].answer,
            Answer::Text(vec![
                "dynamic retention x".into(),
                "2,500 pounds".into(),
                "24/7".into(),
                "Smith, John".into()
            ])
        );
    }

    #[test]
    fn picking_a_letter_converts_a_wrong_answer_kind() {
        let mut original = task(
            TaskKind::MultipleChoice { options: 3 },
            vec![Answer::Text(vec!["B".into()])],
        );
        original.items[0].options = "ABC"
            .chars()
            .map(|letter| Choice {
                letter,
                text: format!("Option {letter}"),
            })
            .collect();
        let mut draft = TaskDraft::from_task(&original);
        assert_eq!(draft.items[0].answer, AnswerDraft::Letter(None));
        assert_eq!(draft.letter_choices(0), vec!['A', 'B', 'C']);
        draft.items[0].answer = AnswerDraft::Letter(Some('B'));
        assert_eq!(draft.to_task().items[0].answer, Answer::Letters(vec!['B']));
        // TFNG sent as text: choosing a value makes it a TFNG key.
        let tfng = task(
            TaskKind::TrueFalseNotGiven,
            vec![Answer::Text(vec!["T".into()])],
        );
        let mut draft = TaskDraft::from_task(&tfng);
        draft.items[0].answer = AnswerDraft::Tfng(Some(Tfng::NotGiven));
        assert_eq!(
            draft.to_task().items[0].answer,
            Answer::Tfng(Tfng::NotGiven)
        );
    }

    #[test]
    fn an_unchosen_answer_keeps_the_original() {
        let original = task(
            TaskKind::TrueFalseNotGiven,
            vec![Answer::Letters(vec!['T'])],
        );
        let mut draft = TaskDraft::from_task(&original);
        draft.instruction = "Decide.".into();
        let edited = draft.to_task();
        assert_eq!(edited.items[0].answer, Answer::Letters(vec!['T']));
        assert_eq!(edited.instruction, "Decide.");
    }

    #[test]
    fn note_text_can_be_written_and_stray_fields_removed() {
        let limit = WordLimit::words_or_number(1);
        let notes = task(
            TaskKind::NoteCompletion(limit),
            vec![
                Answer::Text(vec!["library".into()]),
                Answer::Text(vec!["15 May".into()]),
            ],
        );
        let mut draft = TaskDraft::from_task(&notes);
        assert_eq!(draft.summary, "");
        draft.summary = "Place: (21)______\nDate: (22)______".into();
        assert_eq!(
            draft.to_task().summary.as_deref(),
            Some("Place: (21)______\nDate: (22)______")
        );
        draft.summary = "   ".into();
        assert_eq!(draft.to_task().summary, None);

        let mut short = task(
            TaskKind::ShortAnswer(limit),
            vec![Answer::Text(vec!["library".into()])],
        );
        short.summary = Some("A stray paragraph.".into());
        let mut draft = TaskDraft::from_task(&short);
        assert_eq!(draft.stray_summary.as_deref(), Some("A stray paragraph."));
        draft.stray_summary = None;
        assert_eq!(draft.to_task().summary, None);
    }

    #[test]
    fn options_are_added_and_removed_in_letter_order() {
        let mut matching = task(
            TaskKind::Matching { options: 3 },
            vec![Answer::Letters(vec!['A'])],
        );
        matching.shared_options = vec![Choice {
            letter: 'A',
            text: "one".into(),
        }];
        let mut draft = TaskDraft::from_task(&matching);
        draft.add_option(None);
        draft.add_option(None);
        let letters: Vec<char> = draft.shared_options.iter().map(|o| o.letter).collect();
        assert_eq!(letters, ['A', 'B', 'C']);
        draft.remove_last_option(None);
        assert_eq!(draft.shared_options.len(), 2);
        // Who-mentioned options are initials: a new one takes a free letter.
        let mut who = task(
            TaskKind::WhoMentioned { guests: 2 },
            vec![Answer::Letters(vec!['S'])],
        );
        who.shared_options = ['S', 'A']
            .map(|letter| Choice {
                letter,
                text: String::new(),
            })
            .to_vec();
        let mut draft = TaskDraft::from_task(&who);
        draft.add_option(None);
        assert_eq!(draft.shared_options[2].letter, 'B');
    }
}
