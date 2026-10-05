use dioxus::prelude::*;
use uuid::Uuid;

use crate::domain::{Accent, Gender, SpeakerConfig, SpeakerRole, ValidationIssue, VoiceChoice};
use crate::ui::components::issue_list::IssueList;
use crate::ui::components::voices::VoicePicker;

/// The speakers of a part as cards: gender, accent and role badges, the
/// voice with its buttons, then the validator's warnings about the line-up.
#[component]
pub fn SpeakerCards(
    speakers: Vec<SpeakerConfig>,
    /// Something of this part is running: no edits until it is done.
    disabled: bool,
    #[props(default)] warnings: Vec<ValidationIssue>,
    /// The saved exam a new voice sample is booked to.
    #[props(default)]
    exam: Option<Uuid>,
    onedit: EventHandler<usize>,
    onvoice: EventHandler<(usize, VoiceChoice)>,
) -> Element {
    rsx! {
        div { class: "speakers-list",
            for (idx, speaker) in speakers.iter().enumerate() {
                div { class: "speaker-card", key: "{speaker.label}",
                    div { class: "speaker-card-header",
                        div { class: "speaker-name", "{speaker.label}" }
                        button {
                            class: "edit-button",
                            disabled,
                            onclick: move |_| onedit.call(idx),
                            "Edit"
                        }
                    }
                    div { class: "speaker-details",
                        span { class: "speaker-badge", "{speaker.gender.label()}" }
                        span { class: "speaker-badge", "{speaker.accent.label()}" }
                        span { class: "speaker-badge role", "{speaker.role.label()}" }
                    }
                    VoicePicker {
                        speaker: speaker.clone(),
                        index: idx,
                        speakers: speakers.clone(),
                        disabled,
                        exam,
                        onchange: move |choice| onvoice.call((idx, choice)),
                    }
                }
            }
        }
        IssueList { issues: warnings }
    }
}

/// Edits gender, accent and role of one speaker. The label is fixed; a new
/// gender or accent sends the speaker back to an automatic voice.
#[component]
pub fn SpeakerEditModal(
    speaker: SpeakerConfig,
    onclose: EventHandler<()>,
    onsave: EventHandler<SpeakerConfig>,
) -> Element {
    let label = speaker.label.clone();
    let mut edited_gender = use_signal(|| speaker.gender);
    let mut edited_accent = use_signal(|| speaker.accent);
    let mut edited_role_key = use_signal(|| speaker.role.key().to_string());
    let mut custom_role_text = use_signal(|| match &speaker.role {
        SpeakerRole::Other(s) => s.clone(),
        _ => String::new(),
    });

    let save_label = label.clone();
    let (saved_gender, saved_accent) = (speaker.gender, speaker.accent);
    let saved_voice = speaker.voice.clone();
    let saved_voice_line = speaker.voice.clone();
    let handle_save = move |_| {
        let (gender, accent) = (edited_gender(), edited_accent());
        // The voice fits the old gender and accent only; after a change the
        // speaker waits for a new one.
        let voice = if (gender, accent) == (saved_gender, saved_accent) {
            saved_voice.clone()
        } else {
            VoiceChoice::Auto
        };
        onsave.call(SpeakerConfig {
            label: save_label.clone(),
            gender,
            accent,
            role: SpeakerRole::from_key(&edited_role_key(), &custom_role_text()),
            voice,
        });
    };

    rsx! {
        div { class: "modal-overlay",
            onclick: move |_| onclose.call(()),

            div { class: "modal-content",
                onclick: move |e| e.stop_propagation(),

                div { class: "modal-header",
                    h3 { "Edit {label}" }
                    button {
                        class: "modal-close",
                        onclick: move |_| onclose.call(()),
                        "\u{d7}"
                    }
                }

                div { class: "modal-body",
                    div { class: "form-group",
                        label { "Gender:" }
                        select {
                            class: "form-select",
                            value: if matches!(edited_gender(), Gender::Male) { "male" } else { "female" },
                            onchange: move |evt| {
                                edited_gender.set(if evt.value() == "male" { Gender::Male } else { Gender::Female });
                            },
                            option { value: "male", selected: edited_gender() == Gender::Male, "Male" }
                            option { value: "female", selected: edited_gender() == Gender::Female, "Female" }
                        }
                    }

                    div { class: "form-group",
                        label { "Accent:" }
                        select {
                            class: "form-select",
                            value: "{edited_accent().key()}",
                            onchange: move |evt| {
                                if let Some(accent) = Accent::from_key(&evt.value()) {
                                    edited_accent.set(accent);
                                }
                            },
                            for accent in Accent::ALL {
                                option { value: "{accent.key()}", selected: accent == edited_accent(), "{accent.label()}" }
                            }
                        }
                    }

                    div { class: "form-group",
                        label { "Role:" }
                        select {
                            class: "form-select",
                            value: "{edited_role_key()}",
                            onchange: move |evt| edited_role_key.set(evt.value()),
                            for key in SpeakerRole::PRESET_KEYS {
                                option { value: "{key}", selected: edited_role_key() == key, "{SpeakerRole::from_key(key, \"\").label()}" }
                            }
                            option { value: "other", selected: edited_role_key() == "other", "Other (custom)" }
                        }
                    }

                    if edited_role_key() == "other" {
                        div { class: "form-group",
                            label { "Custom role:" }
                            input {
                                class: "form-input",
                                r#type: "text",
                                placeholder: "e.g. Customer, Journalist, Mayor",
                                value: "{custom_role_text()}",
                                oninput: move |evt| custom_role_text.set(evt.value()),
                            }
                        }
                    }

                    p { class: "muted",
                        "Role shapes the script and the delivery; the voice comes from gender and accent."
                    }
                    p { class: "muted",
                        if (edited_gender(), edited_accent()) != (saved_gender, saved_accent) {
                            "A {edited_accent().label()} {edited_gender().label().to_lowercase()} voice is picked when you save."
                        } else if let Some(voice) = saved_voice_line.voice() {
                            "Voice: {voice.display_name()}"
                        } else {
                            "The voice is picked automatically."
                        }
                    }
                }

                div { class: "modal-footer",
                    button { class: "button-secondary", onclick: move |_| onclose.call(()), "Cancel" }
                    button { class: "button-primary", onclick: handle_save, "Save changes" }
                }
            }
        }
    }
}
