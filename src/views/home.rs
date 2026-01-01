//! Home view - IELTS Listening Practice Exercise Generator

use crate::domain::{Accent, GenerationRequest, ListeningSection, SpeakerRole};
use crate::services::{audio_job_manager, script_generator, topic_generator};
use dioxus::prelude::*;

#[cfg(target_arch = "wasm32")]
use gloo_timers::future::TimeoutFuture;

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
    let mut topic_error = use_signal(|| None::<String>);
    let mut is_generating_topic = use_signal(|| false);

    // State for generated script
    let mut generated_script = use_signal(|| None::<String>);
    let mut generation_error = use_signal(|| None::<String>);
    let mut is_generating_audio = use_signal(|| false);
    let mut audio_error = use_signal(|| None::<String>);

    // State for generated audio
    let mut generated_audio = use_signal(|| None::<Vec<u8>>);
    let mut is_audio_playing = use_signal(|| false);

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

    // Handle topic auto-generation
    let handle_generate_topic = move |_| {
        topic_error.set(None);
        is_generating_topic.set(true);

        let section_str = match selected_section() {
            ListeningSection::Section1 => "Section 1",
            ListeningSection::Section2 => "Section 2",
            ListeningSection::Section3 => "Section 3",
            ListeningSection::Section4 => "Section 4",
        };

        spawn(async move {
            match topic_generator::generate_topic_suggestion(section_str.to_string()).await {
                Ok(generated_topic) => {
                    topic.set(generated_topic);
                    topic_error.set(None);
                }
                Err(e) => {
                    topic_error.set(Some(format!("Failed to generate topic: {}", e)));
                }
            }
            is_generating_topic.set(false);
        });
    };

    // Handle generation
    let handle_generate = move |_| {
        // Validate topic
        if topic().trim().is_empty() {
            generation_error.set(Some("Please enter a topic description".to_string()));
            return;
        }

        // Create request
        let request = GenerationRequest {
            section: selected_section(),
            topic: topic(),
        };

        // Validate request
        if let Err(e) = request.validate() {
            generation_error.set(Some(e));
            return;
        }

        // Call generation service
        generation_error.set(None);
        is_generating.set(true);
        generation_success.set(false);
        generated_script.set(None);

        let current_speakers = speakers();
        let current_section = selected_section();
        let current_topic = topic();

        spawn(async move {
            match script_generator::generate_script(
                current_section,
                current_topic,
                current_speakers.clone(),
            )
            .await
            {
                Ok(script) => {
                    generated_script.set(Some(script));
                    generation_success.set(true);
                    generation_error.set(None);
                }
                Err(e) => {
                    generation_error.set(Some(format!("Failed to generate script: {}", e)));
                    generation_success.set(false);
                }
            }
            is_generating.set(false);
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
                            value: "{topic}",
                            oninput: move |evt| {
                                topic.set(evt.value());
                                topic_error.set(None);
                            },
                        }

                        button {
                            class: "auto-generate-button",
                            disabled: is_generating_topic(),
                            onclick: handle_generate_topic,
                            title: "Auto-generate topic using AI",

                            if is_generating_topic() {
                                "✨ Generating..."
                            } else {
                                "✨ Auto-generate"
                            }
                        }
                    }

                    if let Some(error) = topic_error() {
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

                    if let Some(error) = generation_error() {
                        div { class: "error-box",
                            span { class: "error-icon", "⚠" }
                            span { "{error}" }
                        }
                    }

                    if generation_success() {
                        if let Some(ref script) = generated_script() {
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
                                            if let Some(script) = generated_script() {
                                                download_script(&script, &format!("IELTS_Listening_{:?}_Script.txt", selected_section()));
                                            }
                                        },
                                        "📄 Download Script"
                                    }

                                    button {
                                        class: "download-button secondary",
                                        disabled: is_generating_audio(),
                                        onclick: move |_| {
                                            let script = match generated_script() {
                                                Some(s) => s,
                                                None => return,
                                            };
                                            let speakers_config = speakers();
                                            let section = selected_section();

                                            is_generating_audio.set(true);
                                            audio_error.set(None);

                                            spawn(async move {
                                                // Start background job and get job ID
                                                match audio_job_manager::start_audio_generation(script.clone(), speakers_config.clone(), section).await {
                                                    Ok(job_id) => {
                                                        // Poll for job completion
                                                        loop {
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
                                                                                    generated_audio.set(Some(audio_data));
                                                                                    audio_error.set(None);
                                                                                }
                                                                                Err(e) => {
                                                                                    audio_error.set(Some(format!("Failed to retrieve audio: {}", e)));
                                                                                }
                                                                            }
                                                                            break;
                                                                        }
                                                                        audio_job_manager::JobStatus::Failed => {
                                                                            audio_error.set(Some(job.error.unwrap_or_else(|| "Audio generation failed".to_string())));
                                                                            break;
                                                                        }
                                                                        _ => {
                                                                            // Still processing, continue polling
                                                                        }
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    audio_error.set(Some(format!("Failed to check job status: {}", e)));
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        audio_error.set(Some(format!("Failed to start audio generation: {}", e)));
                                                    }
                                                }
                                                is_generating_audio.set(false);
                                            });
                                        },
                                        if is_generating_audio() {
                                            "🎵 Generating Audio..."
                                        } else {
                                            "🎵 Generate Audio"
                                        }
                                    }
                                }

                                if let Some(error) = audio_error() {
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
                                if let Some(audio_data) = generated_audio() {
                                    div { class: "audio-player-section",
                                        div { class: "audio-player",
                                            audio {
                                                id: "audio-element",
                                                controls: true,
                                                src: create_audio_url(&audio_data),
                                                onplay: move |_| is_audio_playing.set(true),
                                                onpause: move |_| is_audio_playing.set(false),
                                                onended: move |_| is_audio_playing.set(false),
                                            }
                                        }
                                        div { class: "audio-controls",
                                            button {
                                                class: "download-button primary small",
                                                onclick: move |_| {
                                                    if let Some(audio) = generated_audio() {
                                                        let section = selected_section();
                                                        download_audio(&audio, &format!("IELTS_Listening_{:?}_Audio.wav", section));
                                                    }
                                                },
                                                "⬇ Download Audio"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if !is_generating() && generation_error().is_none() {
                        div { class: "empty-state",
                            p { "Results will appear here after generation." }
                        }
                    }

                    if is_generating() {
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
                    disabled: is_generating(),
                    onclick: handle_generate,

                    if is_generating() {
                        "Generating..."
                    } else {
                        "Generate Exercise"
                    }
                }
            }

            // Loading Popups
            if is_generating_topic() {
                LoadingPopup {
                    message: "Generating Topic...",
                    submessage: "Creating a relevant topic suggestion",
                    oncancel: move |_| {
                        is_generating_topic.set(false);
                        topic_error.set(Some("Topic generation cancelled".to_string()));
                    }
                }
            }

            if is_generating() {
                LoadingPopup {
                    message: "Generating Script...",
                    submessage: "This may take 30-60 seconds",
                    oncancel: move |_| {
                        is_generating.set(false);
                        generation_error.set(Some("Script generation cancelled".to_string()));
                    }
                }
            }

            if is_generating_audio() {
                LoadingPopup {
                    message: "Generating Audio...",
                    submessage: "This may take 2-5 minutes for complex scripts. Please wait...",
                    oncancel: move |_| {
                        is_generating_audio.set(false);
                        audio_error.set(Some("Audio generation cancelled".to_string()));
                    }
                }
            }
        }
    }
}

// Loading Popup Component
#[component]
fn LoadingPopup(message: String, submessage: String, oncancel: EventHandler<()>) -> Element {
    rsx! {
        div { class: "loading-popup-container",
            div { class: "loading-popup-content",
                div { class: "loading-popup-header",
                    div { class: "loading-popup-header-content",
                        div { class: "loading-popup-spinner" }
                        div { class: "loading-popup-text-content",
                            p { class: "loading-popup-text", "{message}" }
                            p { class: "loading-popup-subtext", "{submessage}" }
                        }
                    }
                    button {
                        class: "loading-popup-close",
                        onclick: move |_| oncancel.call(()),
                        title: "Cancel generation",
                        "✕"
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
    let mut custom_role_text = use_signal(|| match &speaker.role {
        SpeakerRole::Other(s) => s.clone(),
        _ => String::new(),
    });

    let handle_save = move |_| {
        let final_role = match edited_role() {
            SpeakerRole::Other(_) => SpeakerRole::Other(custom_role_text()),
            other => other,
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

// Helper function to download audio as WAV file
#[cfg(target_arch = "wasm32")]
fn download_audio(audio_data: &[u8], filename: &str) {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url, window};

    if let Some(window) = window() {
        if let Some(document) = window.document() {
            // Create Uint8Array from audio data
            let uint8_array = js_sys::Uint8Array::new_with_length(audio_data.len() as u32);
            uint8_array.copy_from(audio_data);

            // Create blob with audio/wav mime type
            let array = js_sys::Array::new();
            array.push(&uint8_array);

            let mut blob_options = BlobPropertyBag::new();
            blob_options.set_type("audio/wav");

            if let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&array, &blob_options) {
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
fn download_audio(_audio_data: &[u8], _filename: &str) {
    // Download not implemented for non-wasm targets
}

// Helper function to create audio blob URL for playback
#[cfg(target_arch = "wasm32")]
fn create_audio_url(audio_data: &[u8]) -> String {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, Url};

    // Create Uint8Array from audio data
    let uint8_array = js_sys::Uint8Array::new_with_length(audio_data.len() as u32);
    uint8_array.copy_from(audio_data);

    // Create blob with audio/wav mime type
    let array = js_sys::Array::new();
    array.push(&uint8_array);

    let mut blob_options = BlobPropertyBag::new();
    blob_options.set_type("audio/wav");

    if let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&array, &blob_options) {
        if let Ok(url) = Url::create_object_url_with_blob(&blob) {
            return url;
        }
    }

    // Return empty string if creation fails
    String::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn create_audio_url(_audio_data: &[u8]) -> String {
    String::new()
}
