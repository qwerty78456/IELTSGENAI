//! Home view - IELTS Listening Practice Exercise Generator

use dioxus::prelude::*;
use crate::domain::{ListeningSection, GenerationRequest};

#[component]
pub fn Home() -> Element {
    // State for the form inputs
    let mut topic = use_signal(|| String::new());
    let mut selected_section = use_signal(|| ListeningSection::Section1);
    let mut generation_success = use_signal(|| false);
    let mut is_generating = use_signal(|| false);

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
                        div { class: "customize-header",
                            span { "Customize speakers ⚙" }
                        }
                        // TODO: Will be implemented later
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
