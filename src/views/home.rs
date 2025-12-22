//! Home view - IELTS Listening Practice Exercise Generator

use dioxus::prelude::*;
use crate::domain::{ListeningSection, GenerationRequest, SpeakerRole, Accent};

#[component]
pub fn Home() -> Element {
    // State for the form inputs
    let mut topic = use_signal(|| String::new());
    let mut selected_section = use_signal(|| ListeningSection::Section1);
    let mut generation_success = use_signal(|| false);
    let mut is_generating = use_signal(|| false);
    let mut show_speakers = use_signal(|| false);
    
    // State for speaker customization
    let mut custom_speakers = use_signal(|| Vec::new());
    let mut editing_speaker_idx = use_signal(|| None::<usize>);
    
    // Compute speakers based on selected section or custom overrides
    let speakers = use_memo(move || {
        if !custom_speakers().is_empty() {
            custom_speakers()
        } else {
            let request = GenerationRequest {
                section: selected_section(),
                topic: "temp".to_string(),
            };
            request.generate_default_speakers()
        }
    });

    // Handle generation
    let handle_generate = move |_| {
        // Validate topic
        if topic().trim().is_empty() {
            return;
        }

        // Create request
        let request = GenerationRequest {
            section: selected_section(),
            topic: topic(),
        };

        // Validate request
        if let Err(_e) = request.validate() {
            // TODO: Show error to user
            return;
        }

        // TODO: Call generation service
        is_generating.set(true);
        
        // Simulate generation (will be replaced with actual service call)
        spawn(async move {
            // Simulate API call delay
            #[cfg(target_arch = "wasm32")]
            {
                use gloo_timers::future::TimeoutFuture;
                TimeoutFuture::new(2000).await;
            }
            
            #[cfg(not(target_arch = "wasm32"))]
            {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            
            is_generating.set(false);
            generation_success.set(true);
        });
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/styling/generator.css") }
        
        div { class: "generator-container",
            // Header
            div { class: "generator-header",
                h1 { "Generate Your Own IELTS Listening Practice Exercise" }
                p { "Create custom listening tests based on your desired content." }
            }

            // Three-column grid
            div { class: "generator-grid",
                // Column 1: Content Input
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Content Input" }
                    
                    label { class: "input-label",
                        "Describe the Audio Scenario:"
                    }
                    
                    textarea {
                        class: "input-textarea",
                        placeholder: "Example: A phone conversation about booking a driving lesson.",
                        value: "{topic}",
                        oninput: move |evt| topic.set(evt.value()),
                    }
                }

                // Column 2: IELTS Configuration
                div { class: "generator-panel",
                    h2 { class: "panel-header", "IELTS Configuration" }
                    
                    div { class: "config-group",
                        label { class: "input-label",
                            "Select Listening Section:"
                        }
                        
                        div { class: "select-wrapper",
                            select {
                                class: "section-select",
                                value: "{section_to_string(selected_section())}",
                                onchange: move |evt| {
                                    selected_section.set(string_to_section(&evt.value()));
                                },
                                
                                option { value: "section1", "Section 1: Transactional Conversation" }
                                option { value: "section2", "Section 2: Guided Monologue" }
                                option { value: "section3", "Section 3: Academic Discussion" }
                                option { value: "section4", "Section 4: Academic Lecture" }
                            }
                        }
                    }

                    div { class: "customize-section",
                        button {
                            class: "customize-toggle",
                            onclick: move |_| show_speakers.set(!show_speakers()),
                            span { "Customize speakers ⚙" }
                            span { class: "toggle-icon", if show_speakers() { "▼" } else { "▶" } }
                        }
                        
                        if show_speakers() {
                            div { class: "speakers-list",
                                for (idx, speaker) in speakers().iter().enumerate() {
                                    {
                                        let gender_str = format!("{:?}", speaker.gender);
                                        let accent_str = format!("{:?}", speaker.accent);
                                        let role_str = match &speaker.role {
                                            crate::domain::SpeakerRole::Other(s) => s.clone(),
                                            _ => format!("{:?}", speaker.role)
                                        };
                                        
                                        rsx! {
                                            div { class: "speaker-card", key: "{idx}",
                                                div { class: "speaker-card-header",
                                                    div { class: "speaker-name", "{speaker.name}" }
                                                    button {
                                                        class: "edit-button",
                                                        onclick: move |_| editing_speaker_idx.set(Some(idx)),
                                                        "✏️ Edit"
                                                    }
                                                }
                                                div { class: "speaker-details",
                                                    span { class: "speaker-badge", "{gender_str}" }
                                                    span { class: "speaker-badge", "{accent_str}" }
                                                    span { class: "speaker-badge role", "{role_str}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Speaker edit modal
                        if let Some(idx) = editing_speaker_idx() {
                            if let Some(speaker) = speakers().get(idx) {
                                SpeakerEditModal {
                                    speaker: speaker.clone(),
                                    onclose: move |_| editing_speaker_idx.set(None),
                                    onsave: move |updated_speaker| {
                                        let mut speakers_vec = speakers().clone();
                                        speakers_vec[idx] = updated_speaker;
                                        custom_speakers.set(speakers_vec);
                                        editing_speaker_idx.set(None);
                                    }
                                }
                            }
                        }
                    }
                }

                // Column 3: Generation Results
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Generation Results" }
                    
                    if generation_success() {
                        div { class: "success-message",
                            span { class: "success-icon", "✓" }
                            span { "Success! Exercise Generated." }
                        }
                    } else {
                        div { class: "empty-state",
                            p { "Results will appear here after generation." }
                        }
                    }
                }
            }

            // Generate Button
            div { class: "generate-button-container",
                button {
                    class: "generate-button",
                    disabled: is_generating(),
                    onclick: handle_generate,
                    
                    if is_generating() {
                        "Generating..."
                    } else {
                        "Generate Exercise"
                    }
                }
            }
        }
    }
}

// Speaker Edit Modal Component
#[component]
fn SpeakerEditModal(
    speaker: crate::domain::SpeakerConfig,
    onclose: EventHandler<()>,
    onsave: EventHandler<crate::domain::SpeakerConfig>,
) -> Element {
    use crate::domain::{Gender, SpeakerRole};
    
    let speaker_name = speaker.name.clone();
    let mut edited_gender = use_signal(|| speaker.gender);
    let mut edited_accent = use_signal(|| speaker.accent);
    let mut edited_role = use_signal(|| speaker.role.clone());
    let mut custom_role_text = use_signal(|| {
        match &speaker.role {
            SpeakerRole::Other(s) => s.clone(),
            _ => String::new()
        }
    });
    
    let handle_save = move |_| {
        let final_role = match edited_role() {
            SpeakerRole::Other(_) => SpeakerRole::Other(custom_role_text()),
            other => other
        };
        
        onsave.call(crate::domain::SpeakerConfig {
            name: speaker.name.clone(),
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

// Helper functions for section conversion
fn section_to_string(section: ListeningSection) -> &'static str {
    match section {
        ListeningSection::Section1 => "section1",
        ListeningSection::Section2 => "section2",
        ListeningSection::Section3 => "section3",
        ListeningSection::Section4 => "section4",
    }
}

fn string_to_section(s: &str) -> ListeningSection {
    match s {
        "section2" => ListeningSection::Section2,
        "section3" => ListeningSection::Section3,
        "section4" => ListeningSection::Section4,
        _ => ListeningSection::Section1,
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
