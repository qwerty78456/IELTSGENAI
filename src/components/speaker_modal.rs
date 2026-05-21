use dioxus::prelude::*;
use crate::domain::{Accent, Gender, SpeakerRole, SpeakerConfig};

#[component]
pub fn SpeakerEditModal(
    speaker: SpeakerConfig,
    onclose: EventHandler<()>,
    onsave: EventHandler<SpeakerConfig>,
) -> Element {
    let speaker_name = speaker.name.clone();
    let mut edited_gender = use_signal(|| speaker.gender);
    let mut edited_accent = use_signal(|| speaker.accent);
    let mut edited_role = use_signal(|| speaker.role.clone());
    let mut custom_role_text = use_signal(|| match &speaker.role {
        SpeakerRole::Other(s) => s.clone(),
        _ => String::new(),
    });

    let save_name = speaker_name.clone();
    let handle_save = move |_| {
        let final_role = match edited_role() {
            SpeakerRole::Other(_) => SpeakerRole::Other(custom_role_text()),
            other => other,
        };

        onsave.call(SpeakerConfig {
            name: save_name.clone(),
            gender: edited_gender(),
            accent: edited_accent(),
            role: final_role,
        });
    };

    rsx! {
        div { class: "modal-overlay",
            onclick: move |_| onclose.call(()),

            div { class: "modal-content",
                onclick: move |e| e.stop_propagation(),

                div { class: "modal-header",
                    h3 { "Edit {speaker_name}" }
                    button {
                        class: "modal-close",
                        onclick: move |_| onclose.call(()),
                        "×"
                    }
                }

                div { class: "modal-body",
                    div { class: "form-group",
                        label { "Gender:" }
                        select {
                            class: "form-select",
                            value: if matches!(edited_gender(), Gender::Male) { "male" } else { "female" },
                            onchange: move |evt| {
                                edited_gender.set(if evt.value() == "male" {
                                    Gender::Male
                                } else {
                                    Gender::Female
                                });
                            },
                            option { value: "male", "Male" }
                            option { value: "female", "Female" }
                        }
                    }

                    div { class: "form-group",
                        label { "Accent:" }
                        select {
                            class: "form-select",
                            value: "{accent_to_string(edited_accent())}",
                            onchange: move |evt| {
                                edited_accent.set(string_to_accent(&evt.value()));
                            },
                            option { value: "british", "British" }
                            option { value: "american", "American" }
                            option { value: "australian", "Australian" }
                            option { value: "canadian", "Canadian" }
                            option { value: "newzealand", "New Zealand" }
                        }
                    }

                    div { class: "form-group",
                        label { "Role:" }
                        select {
                            class: "form-select",
                            value: "{role_to_string(&edited_role())}",
                            onchange: move |evt| {
                                edited_role.set(string_to_role(&evt.value()));
                            },
                            option { value: "student", "Student" }
                            option { value: "professor", "Professor" }
                            option { value: "clerk", "Clerk" }
                            option { value: "receptionist", "Receptionist" }
                            option { value: "guide", "Guide" }
                            option { value: "other", "Other" }
                        }
                    }

                    if matches!(edited_role(), SpeakerRole::Other(_)) {
                        div { class: "form-group",
                            label { "Custom Role:" }
                            input {
                                class: "form-input",
                                r#type: "text",
                                value: "{custom_role_text}",
                                oninput: move |evt| custom_role_text.set(evt.value()),
                                placeholder: "Enter custom role..."
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
                        onclick: handle_save,
                        "Save Changes"
                    }
                }
            }
        }
    }
}

// Helper functions for accent
fn accent_to_string(accent: Accent) -> &'static str {
    match accent {
        Accent::British => "british",
        Accent::American => "american",
        Accent::Australian => "australian",
        Accent::Canadian => "canadian",
        Accent::NewZealand => "newzealand",
    }
}

fn string_to_accent(s: &str) -> Accent {
    match s {
        "american" => Accent::American,
        "australian" => Accent::Australian,
        "canadian" => Accent::Canadian,
        "newzealand" => Accent::NewZealand,
        _ => Accent::British,
    }
}

// Helper functions for role
fn role_to_string(role: &SpeakerRole) -> &'static str {
    match role {
        SpeakerRole::Student => "student",
        SpeakerRole::Professor => "professor",
        SpeakerRole::Clerk => "clerk",
        SpeakerRole::Receptionist => "receptionist",
        SpeakerRole::Guide => "guide",
        SpeakerRole::Other(_) => "other",
    }
}

fn string_to_role(s: &str) -> SpeakerRole {
    match s {
        "student" => SpeakerRole::Student,
        "professor" => SpeakerRole::Professor,
        "clerk" => SpeakerRole::Clerk,
        "receptionist" => SpeakerRole::Receptionist,
        "guide" => SpeakerRole::Guide,
        "other" => SpeakerRole::Other(String::new()),
        _ => SpeakerRole::Student,
    }
}
