use dioxus::prelude::*;
use uuid::Uuid;

use crate::domain::{
    Accent, Gender, SpeakerConfig, SpeakerRole, ValidationIssue, Voice, VoiceChoice,
};
use crate::ui::components::issue_list::IssueList;
use crate::ui::components::voices::{DesignedVoicesPanel, VoicePicker};

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
    /// A voice sample came back (the first of a voice is billed).
    #[props(default)]
    onspend: EventHandler<()>,
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
                        onspend,
                    }
                }
            }
        }
        IssueList { issues: warnings }
    }
}

/// Edits gender, accent and role of one speaker, or gives it a designed
/// voice (listed, and made, in the "Designed voices" section). The label is
/// fixed; a new gender or accent sends the speaker back to an automatic
/// voice, and a designed voice brings its own gender and accent.
#[component]
pub fn SpeakerEditModal(
    speaker: SpeakerConfig,
    /// Voice ids the other speakers of the part have.
    #[props(default)]
    taken: Vec<String>,
    /// The saved exam a new designed voice is booked to.
    #[props(default)]
    exam: Option<Uuid>,
    onclose: EventHandler<()>,
    onsave: EventHandler<SpeakerConfig>,
    /// A voice was designed (billed).
    #[props(default)]
    onspend: EventHandler<()>,
) -> Element {
    let label = speaker.label.clone();
    let mut edited_gender = use_signal(|| speaker.gender);
    let mut edited_accent = use_signal(|| speaker.accent);
    // A designed voice picked in this dialog, saved with the speaker.
    let mut chosen = use_signal(|| None::<Voice>);
    // The speaker's saved voice was deleted in this dialog: it gets a new one.
    let mut dropped = use_signal(|| false);
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
        let voice = if (gender, accent) == (saved_gender, saved_accent) && !dropped() {
            saved_voice.clone()
        } else {
            VoiceChoice::Auto
        };
        let edited = SpeakerConfig {
            label: save_label.clone(),
            gender,
            accent,
            role: SpeakerRole::from_key(&edited_role_key(), &custom_role_text()),
            voice,
        };
        onsave.call(match chosen() {
            Some(designed) => edited.with_voice(designed),
            None => edited,
        });
    };
    let unchanged_voice = speaker.voice_id().map(str::to_string);
    let deleted_from = unchanged_voice.clone();
    let current_voice = match chosen() {
        Some(voice) => Some(voice.id),
        None if (edited_gender(), edited_accent()) == (saved_gender, saved_accent)
            && !dropped() =>
        {
            unchanged_voice
        }
        None => None,
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
                                let gender = if evt.value() == "male" { Gender::Male } else { Gender::Female };
                                edited_gender.set(gender);
                                if chosen.peek().as_ref().is_some_and(|v| v.gender != gender) {
                                    chosen.set(None);
                                }
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
                                    if chosen.peek().as_ref().is_some_and(|v| v.accent != accent) {
                                        chosen.set(None);
                                    }
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
                        if let Some(voice) = chosen() {
                            "Voice: {voice.display_name()} (designed), kept when you save."
                        } else if (edited_gender(), edited_accent()) != (saved_gender, saved_accent)
                            || dropped()
                        {
                            "A new {edited_gender().label().to_lowercase()} {edited_accent().label()} voice is picked when you save."
                        } else if let Some(voice) = saved_voice_line.voice() {
                            "Voice: {voice.display_name()}"
                        } else {
                            "The voice is picked automatically."
                        }
                    }

                    DesignedVoicesPanel {
                        label: label.clone(),
                        gender: edited_gender(),
                        accent: edited_accent(),
                        current: current_voice,
                        taken: taken.clone(),
                        exam,
                        onchoose: move |voice: Voice| {
                            edited_gender.set(voice.gender);
                            edited_accent.set(voice.accent);
                            chosen.set(Some(voice));
                        },
                        ondelete: move |id: String| {
                            if deleted_from.as_deref() == Some(id.as_str()) {
                                dropped.set(true);
                            }
                            if chosen.peek().as_ref().is_some_and(|v| v.id == id) {
                                chosen.set(None);
                            }
                        },
                        onspend,
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
