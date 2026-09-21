use dioxus::prelude::*;

use crate::domain::{Accent, Gender, SpeakerConfig, SpeakerRole};

/// Edits gender, accent and role of one speaker. The label is fixed.
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
    let handle_save = move |_| {
        onsave.call(SpeakerConfig {
            label: save_label.clone(),
            gender: edited_gender(),
            accent: edited_accent(),
            role: SpeakerRole::from_key(&edited_role_key(), &custom_role_text()),
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
                            option { value: "male", "Male" }
                            option { value: "female", "Female" }
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
                                option { value: "{accent.key()}", "{accent.label()}" }
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
                                option { value: "{key}", "{SpeakerRole::from_key(key, \"\").label()}" }
                            }
                            option { value: "other", "Other (custom)" }
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
                }

                div { class: "modal-footer",
                    button { class: "button-secondary", onclick: move |_| onclose.call(()), "Cancel" }
                    button { class: "button-primary", onclick: handle_save, "Save changes" }
                }
            }
        }
    }
}
