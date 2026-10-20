//! The script editor: the teacher edits a part's script as the page shows
//! it ("Speaker A: ..." per turn). What is being typed lives in the page
//! state (`ScriptEdit`), so it survives a visit to the other page, and is
//! checked against the passage it started from, so it can never overwrite a
//! newer script.

use dioxus::prelude::*;

use crate::domain::Passage;

/// An open script editor: the passage it started from and the text so far.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptEdit {
    pub base: Passage,
    pub text: String,
    /// Why the last Apply was refused.
    pub error: Option<String>,
}

impl ScriptEdit {
    pub fn open(passage: &Passage) -> Self {
        Self {
            base: passage.clone(),
            text: passage.script_text(),
            error: None,
        }
    }

    /// The edited passage, with the part, topic and line-up of the one it
    /// started from; `None` when nothing changed. Refused when the script
    /// was replaced meanwhile (`current` is not `base`) or the text does not
    /// read as turns of `labels` (`Passage::parse_edited`).
    pub fn apply(
        &self,
        current: Option<&Passage>,
        labels: &[String],
    ) -> Result<Option<Passage>, String> {
        if current != Some(&self.base) {
            return Err(
                "The script was replaced while the editor was open; your text is kept here. \
                 Cancel to see the new script."
                    .into(),
            );
        }
        if self.text == self.base.script_text() {
            return Ok(None);
        }
        let parsed =
            Passage::parse_edited(self.base.part, self.base.topic.clone(), &self.text, labels)
                .map_err(|e| e.to_string())?;
        let edited = Passage {
            written_for: self.base.written_for.clone(),
            ..parsed
        };
        Ok((edited.lines != self.base.lines).then_some(edited))
    }
}

/// The editor itself: a text box, the last refusal, Apply and Cancel. The
/// page keeps the text (`ScriptEdit`) and decides what Apply does.
#[component]
pub fn ScriptEditor(
    text: String,
    error: Option<String>,
    disabled: bool,
    oninput: EventHandler<String>,
    onapply: EventHandler<()>,
    oncancel: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "script-edit-box",
            p { class: "muted",
                "One turn per line, starting with its speaker (Speaker A: ...). Speech tags such as <sigh> may stay."
            }
            textarea {
                class: "input-textarea script-edit-area",
                "aria-label": "Script",
                value: "{text}",
                oninput: move |evt| oninput.call(evt.value()),
            }
            if let Some(error) = error {
                div { class: "error-box", role: "alert",
                    span { class: "error-icon", "!" }
                    span { "{error}" }
                }
            }
            div { class: "editor-actions",
                button {
                    class: "button-primary",
                    disabled,
                    onclick: move |_| onapply.call(()),
                    "Apply script"
                }
                button {
                    class: "button-secondary",
                    onclick: move |_| oncancel.call(()),
                    "Cancel"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Accent, Gender, SpeakerConfig, SpeakerRole};

    fn labels() -> Vec<String> {
        vec!["Speaker A".to_string(), "Speaker B".to_string()]
    }

    fn passage() -> Passage {
        Passage::parse(
            1,
            "Booking a room",
            "Speaker A: Good morning.\nSpeaker B: Hello, I'd like a room.",
            &labels(),
        )
        .unwrap()
        .for_speakers(&[SpeakerConfig::new(
            "Speaker A",
            Gender::Female,
            Accent::British,
            SpeakerRole::Receptionist,
        )])
    }

    #[test]
    fn unchanged_text_is_a_no_op() {
        let base = passage();
        let edit = ScriptEdit::open(&base);
        assert_eq!(edit.apply(Some(&base), &labels()), Ok(None));
        // Spacing that parses to the same turns changes nothing either.
        let spaced = ScriptEdit {
            text: format!("{}\n\n", base.script_text()),
            ..edit
        };
        assert_eq!(spaced.apply(Some(&base), &labels()), Ok(None));
    }

    #[test]
    fn apply_keeps_part_topic_and_written_for() {
        let base = passage();
        let edit = ScriptEdit {
            text: "Speaker A: Good afternoon.\nSpeaker B: Hello, I'd like a room.".into(),
            ..ScriptEdit::open(&base)
        };
        let edited = edit.apply(Some(&base), &labels()).unwrap().unwrap();
        assert_eq!(edited.lines[0].text, "Good afternoon.");
        assert_eq!(
            (edited.part, edited.topic.as_str(), &edited.written_for),
            (base.part, base.topic.as_str(), &base.written_for)
        );
    }

    #[test]
    fn apply_after_the_script_was_replaced_is_refused() {
        let base = passage();
        let edit = ScriptEdit {
            text: "Speaker A: Edited.".into(),
            ..ScriptEdit::open(&base)
        };
        let mut newer = base.clone();
        newer.lines[0].text = "A newer script.".into();
        let error = edit.apply(Some(&newer), &labels()).unwrap_err();
        assert!(
            error.contains("replaced while the editor was open"),
            "{error}"
        );
        assert!(edit.apply(None, &labels()).is_err());
    }

    #[test]
    fn unknown_label_is_reported_not_merged() {
        let base = passage();
        let edit = ScriptEdit {
            text: "Speaker A: Hi.\nSpeaker C: Me too.".into(),
            ..ScriptEdit::open(&base)
        };
        let error = edit.apply(Some(&base), &labels()).unwrap_err();
        assert!(error.starts_with("Line 2"), "{error}");
        let empty = ScriptEdit {
            text: String::new(),
            ..ScriptEdit::open(&base)
        };
        assert_eq!(
            empty.apply(Some(&base), &labels()),
            Err("The script is empty".into())
        );
    }
}
