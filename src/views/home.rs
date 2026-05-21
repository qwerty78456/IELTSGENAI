//! Home view - IELTS Listening Practice Exercise Generator

use crate::domain::{GenerationRequest, ListeningSection, SpeakerConfig};
use crate::services::{audio_job_manager, script_generator, topic_generator};
use dioxus::prelude::*;

use crate::components::loading_popup::LoadingPopup;
use crate::components::speaker_modal::SpeakerEditModal;
use crate::components::audio_player::AudioPlayerSection;

#[cfg(target_arch = "wasm32")]
use gloo_timers::future::TimeoutFuture;

#[derive(Clone)]
pub struct HomeState {
    pub topic: String,
    pub topic_error: Option<String>,
    pub is_generating_topic: bool,
    
    pub selected_section: ListeningSection,
    pub custom_speakers: Vec<SpeakerConfig>,
    pub show_speakers: bool,
    pub editing_speaker_idx: Option<usize>,
    
    pub is_generating_script: bool,
    pub generation_success: bool,
    pub generation_error: Option<String>,
    pub generated_script: Option<String>,
    
    pub is_generating_audio: bool,
    pub audio_error: Option<String>,
    pub generated_audio: Option<Vec<u8>>,
}

impl Default for HomeState {
    fn default() -> Self {
        Self {
            topic: String::new(),
            topic_error: None,
            is_generating_topic: false,
            selected_section: ListeningSection::Section1,
            custom_speakers: Vec::new(),
            show_speakers: false,
            editing_speaker_idx: None,
            is_generating_script: false,
            generation_success: false,
            generation_error: None,
            generated_script: None,
            is_generating_audio: false,
            audio_error: None,
            generated_audio: None,
        }
    }
}

#[component]
pub fn Home() -> Element {
    let mut state = use_signal(|| HomeState::default());

    // Compute speakers based on selected section or custom overrides
    let speakers = use_memo(move || {
        let current_state = state();
        if !current_state.custom_speakers.is_empty() {
            current_state.custom_speakers.clone()
        } else {
            let request = GenerationRequest {
                section: current_state.selected_section,
                topic: "temp".to_string(),
            };
            request.generate_default_speakers()
        }
    });

    // Handle topic auto-generation
    let handle_generate_topic = move |_| {
        state.write().topic_error = None;
        state.write().is_generating_topic = true;

        let section_str = match state().selected_section {
            ListeningSection::Section1 => "Section 1",
            ListeningSection::Section2 => "Section 2",
            ListeningSection::Section3 => "Section 3",
            ListeningSection::Section4 => "Section 4",
        };

        spawn(async move {
            match topic_generator::generate_topic_suggestion(section_str.to_string()).await {
                Ok(generated_topic) => {
                    state.write().topic = generated_topic;
                    state.write().topic_error = None;
                }
                Err(e) => {
                    state.write().topic_error = Some(format!("Failed to generate topic: {}", e));
                }
            }
            state.write().is_generating_topic = false;
        });
    };

    // Handle generation
    let handle_generate = move |_| {
        let current_state = state();
        
        // Validate topic
        if current_state.topic.trim().is_empty() {
            state.write().generation_error = Some("Please enter a topic description".to_string());
            return;
        }

        // Create request
        let request = GenerationRequest {
            section: current_state.selected_section,
            topic: current_state.topic.clone(),
        };

        // Validate request
        if let Err(e) = request.validate() {
            state.write().generation_error = Some(e);
            return;
        }

        // Call generation service
        state.write().generation_error = None;
        state.write().is_generating_script = true;
        state.write().generation_success = false;
        state.write().generated_script = None;

        let current_speakers = speakers();
        let current_section = state().selected_section;
        let current_topic = state().topic.clone();

        spawn(async move {
            match script_generator::generate_script(
                current_section,
                current_topic,
                current_speakers.clone(),
            )
            .await
            {
                Ok(script) => {
                    state.write().generated_script = Some(script);
                    state.write().generation_success = true;
                    state.write().generation_error = None;
                }
                Err(e) => {
                    state.write().generation_error = Some(format!("Failed to generate script: {}", e));
                    state.write().generation_success = false;
                }
            }
            state.write().is_generating_script = false;
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

                    div { class: "textarea-container",
                        textarea {
                            class: "input-textarea",
                            placeholder: "Example: A phone conversation about booking a driving lesson.",
                            value: "{state().topic}",
                            oninput: move |evt| {
                                state.write().topic = evt.value();
                                state.write().topic_error = None;
                            },
                        }

                        button {
                            class: "auto-generate-button",
                            disabled: state().is_generating_topic,
                            onclick: handle_generate_topic,
                            title: "Auto-generate topic using AI",

                            if state().is_generating_topic {
                                "✨ Generating..."
                            } else {
                                "✨ Auto-generate"
                            }
                        }
                    }

                    if let Some(error) = state().topic_error.clone() {
                        div { class: "error-message",
                            "{error}"
                        }
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
                                value: "{section_to_string(state().selected_section)}",
                                onchange: move |evt| {
                                    state.write().selected_section = string_to_section(&evt.value());
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
                            onclick: move |_| {
                                let current = state().show_speakers;
                                state.write().show_speakers = !current;
                            },
                            span { "Customize speakers ⚙" }
                            span { class: "toggle-icon", if state().show_speakers { "▼" } else { "▶" } }
                        }

                        if state().show_speakers {
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
                                                        onclick: move |_| state.write().editing_speaker_idx = Some(idx),
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
                        if let Some(idx) = state().editing_speaker_idx {
                            if let Some(speaker) = speakers().get(idx) {
                                SpeakerEditModal {
                                    speaker: speaker.clone(),
                                    onclose: move |_| state.write().editing_speaker_idx = None,
                                    onsave: move |updated_speaker| {
                                        let mut speakers_vec = speakers().clone();
                                        speakers_vec[idx] = updated_speaker;
                                        state.write().custom_speakers = speakers_vec;
                                        state.write().editing_speaker_idx = None;
                                    }
                                }
                            }
                        }
                    }
                }

                // Column 3: Generation Results
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Generation Results" }

                    if let Some(error) = state().generation_error.clone() {
                        div { class: "error-box",
                            span { class: "error-icon", "⚠" }
                            span { "{error}" }
                        }
                    }

                    if state().generation_success {
                        if let Some(ref script) = state().generated_script {
                            div { class: "script-result",
                                div { class: "success-banner",
                                    span { class: "success-icon", "✓" }
                                    span { "Script Generated Successfully!" }
                                }

                                div { class: "script-preview",
                                    h3 { "Script Preview:" }
                                    pre { class: "script-content", "{script}" }
                                }

                                div { class: "download-buttons",
                                    button {
                                        class: "download-button primary",
                                        onclick: move |_| {
                                            if let Some(script) = state().generated_script.clone() {
                                                download_script(&script, &format!("IELTS_Listening_{:?}_Script.txt", state().selected_section));
                                            }
                                        },
                                        "📄 Download Script"
                                    }

                                    button {
                                        class: "download-button secondary",
                                        disabled: state().is_generating_audio,
                                        onclick: move |_| {
                                            let script = match state().generated_script.clone() {
                                                Some(s) => s,
                                                None => return,
                                            };
                                            let speakers_config = speakers();
                                            let section = state().selected_section;

                                            state.write().is_generating_audio = true;
                                            state.write().audio_error = None;

                                            spawn(async move {
                                                // Start background job and get job ID
                                                match audio_job_manager::start_audio_generation(script.clone(), speakers_config.clone(), section).await {
                                                    Ok(job_id) => {
                                                        // Poll for job completion (max 5 minutes)
                                                        let mut poll_attempts = 0u32;
                                                        let max_poll_attempts = 150u32; // 150 × 2s = 5 minutes
                                                        loop {
                                                            poll_attempts += 1;
                                                            if poll_attempts > max_poll_attempts {
                                                                state.write().audio_error = Some("Audio generation timed out after 5 minutes. Please try again.".to_string());
                                                                break;
                                                            }
                                                            // Wait 2 seconds between polls
                                                            #[cfg(target_arch = "wasm32")]
                                                            TimeoutFuture::new(2000).await;

                                                            #[cfg(not(target_arch = "wasm32"))]
                                                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                                                            // Check job status
                                                            match audio_job_manager::check_audio_job_status(job_id.clone()).await {
                                                                Ok(job) => {
                                                                    match job.status {
                                                                        audio_job_manager::JobStatus::Completed => {
                                                                            // Fetch the audio data
                                                                            match audio_job_manager::get_audio_job_result(job_id.clone()).await {
                                                                                Ok(audio_data) => {
                                                                                    state.write().generated_audio = Some(audio_data);
                                                                                    state.write().audio_error = None;
                                                                                }
                                                                                Err(e) => {
                                                                                    state.write().audio_error = Some(format!("Failed to retrieve audio: {}", e));
                                                                                }
                                                                            }
                                                                            break;
                                                                        }
                                                                        audio_job_manager::JobStatus::Failed => {
                                                                            state.write().audio_error = Some(job.error.unwrap_or_else(|| "Audio generation failed".to_string()));
                                                                            break;
                                                                        }
                                                                        _ => {
                                                                            // Still processing, continue polling
                                                                        }
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    state.write().audio_error = Some(format!("Failed to check job status: {}", e));
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        state.write().audio_error = Some(format!("Failed to start audio generation: {}", e));
                                                    }
                                                }
                                                state.write().is_generating_audio = false;
                                            });
                                        },
                                        if state().is_generating_audio {
                                            "🎵 Generating Audio..."
                                        } else {
                                            "🎵 Generate Audio"
                                        }
                                    }
                                }

                                if let Some(error) = state().audio_error.clone() {
                                    div { class: "error-box",
                                        span { class: "error-icon", "⚠" }
                                        span { "{error}" }
                                    }
                                }

                                div { class: "info-box",
                                    p { class: "info-title", "🎙️ Audio Generation" }
                                    p {
                                        "Click 'Generate Audio' to create a high-quality audio file using Gemini 2.5 TTS. "
                                        "Once generated, you can play it directly or download it as a WAV file."
                                    }
                                }

                                // Audio Player Section
                                if let Some(audio_data) = state().generated_audio.clone() {
                                    AudioPlayerSection {
                                        audio_data: audio_data,
                                        section: state().selected_section,
                                    }
                                }
                            }
                        }
                    } else if !state().is_generating_script && state().generation_error.is_none() {
                        div { class: "empty-state",
                            p { "Results will appear here after generation." }
                        }
                    }

                    if state().is_generating_script {
                        div { class: "loading-state",
                            div { class: "spinner" }
                            p { "Generating script... This may take 30-60 seconds." }
                        }
                    }
                }
            }

            // Generate Button
            div { class: "generate-button-container",
                button {
                    class: "generate-button",
                    disabled: state().is_generating_script,
                    onclick: handle_generate,

                    if state().is_generating_script {
                        "Generating..."
                    } else {
                        "Generate Exercise"
                    }
                }
            }

            // Loading Popups
            if state().is_generating_topic {
                LoadingPopup {
                    message: "Generating Topic...".to_string(),
                    submessage: "Creating a relevant topic suggestion".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_topic = false;
                        state.write().topic_error = Some("Topic generation cancelled".to_string());
                    }
                }
            }

            if state().is_generating_script {
                LoadingPopup {
                    message: "Generating Script...".to_string(),
                    submessage: "This may take 30-60 seconds".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_script = false;
                        state.write().generation_error = Some("Script generation cancelled".to_string());
                    }
                }
            }

            if state().is_generating_audio {
                LoadingPopup {
                    message: "Generating Audio...".to_string(),
                    submessage: "This may take 2-5 minutes for complex scripts. Please wait...".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_audio = false;
                        state.write().audio_error = Some("Audio generation cancelled".to_string());
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

// Helper function to download script as text file
#[cfg(target_arch = "wasm32")]
fn download_script(content: &str, filename: &str) {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url, window};

    if let Some(window) = window() {
        if let Some(document) = window.document() {
            // Create blob
            let array = js_sys::Array::new();
            array.push(&wasm_bindgen::JsValue::from_str(content));

            let mut blob_options = BlobPropertyBag::new();
            blob_options.set_type("text/plain;charset=utf-8");

            if let Ok(blob) = Blob::new_with_str_sequence_and_options(&array, &blob_options) {
                if let Ok(url) = Url::create_object_url_with_blob(&blob) {
                    // Create temporary anchor element
                    if let Ok(anchor) = document.create_element("a") {
                        let anchor: HtmlAnchorElement = anchor.unchecked_into();
                        anchor.set_href(&url);
                        anchor.set_download(filename);
                        anchor.click();

                        // Clean up
                        let _ = Url::revoke_object_url(&url);
                    }
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn download_script(_content: &str, _filename: &str) {
    // Download not implemented for non-wasm targets
}
