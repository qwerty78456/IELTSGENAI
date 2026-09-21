//! Home view: choose a format and a part, then generate script, questions and audio.
//!
//! The view holds one `HomeState` signal. Every server call goes through
//! `crate::application`; every rule check is already done by the domain, the
//! view only displays issues.

use dioxus::prelude::*;

use crate::application::audio::{audio_job_result, audio_job_status, start_part_audio, JobStatus};
use crate::application::passages::generate_passage;
use crate::application::tasks::generate_task;
use crate::application::topics::suggest_topic;
use crate::domain::{
    AudioRequest, FormatId, Passage, PassageRequest, SpeakerConfig, Task, TaskRequest, ValidationIssue,
};
use crate::export::markdown;
use crate::ui::components::audio_player::{download_text, AudioPlayerSection};
use crate::ui::components::loading_popup::LoadingPopup;
use crate::ui::components::speaker_modal::SpeakerEditModal;

#[cfg(target_arch = "wasm32")]
use gloo_timers::future::TimeoutFuture;

/// Polling cadence for audio jobs.
const POLL_INTERVAL_MS: u32 = 2_000;
const MAX_POLLS: u32 = 150;

#[derive(Clone)]
pub struct HomeState {
    pub format: FormatId,
    pub part: u8,
    pub topic: String,
    pub topic_error: Option<String>,
    pub is_generating_topic: bool,

    pub custom_speakers: Vec<SpeakerConfig>,
    pub show_speakers: bool,
    pub editing_speaker_idx: Option<usize>,

    pub is_generating_script: bool,
    pub script_error: Option<String>,
    pub passage: Option<Passage>,
    pub passage_issues: Vec<ValidationIssue>,

    pub is_generating_tasks: bool,
    pub tasks_error: Option<String>,
    pub tasks: Vec<Task>,
    pub task_issues: Vec<ValidationIssue>,

    pub is_generating_audio: bool,
    pub audio_error: Option<String>,
    pub generated_audio: Option<Vec<u8>>,
}

impl Default for HomeState {
    fn default() -> Self {
        Self::for_format(FormatId::HsgNational)
    }
}

impl HomeState {
    fn for_format(format: FormatId) -> Self {
        Self {
            format,
            part: 1,
            topic: String::new(),
            topic_error: None,
            is_generating_topic: false,
            custom_speakers: Vec::new(),
            show_speakers: false,
            editing_speaker_idx: None,
            is_generating_script: false,
            script_error: None,
            passage: None,
            passage_issues: Vec::new(),
            is_generating_tasks: false,
            tasks_error: None,
            tasks: Vec::new(),
            task_issues: Vec::new(),
            is_generating_audio: false,
            audio_error: None,
            generated_audio: None,
        }
    }

    /// Everything derived from the current part, kept; the rest cleared.
    fn reset_results(&mut self) {
        self.script_error = None;
        self.passage = None;
        self.passage_issues.clear();
        self.tasks_error = None;
        self.tasks.clear();
        self.task_issues.clear();
        self.audio_error = None;
        self.generated_audio = None;
    }
}

#[component]
pub fn Home() -> Element {
    let mut state = use_signal(HomeState::default);

    let speakers = use_memo(move || {
        let current = state();
        if !current.custom_speakers.is_empty() {
            return current.custom_speakers.clone();
        }
        current.format.format().part(current.part).map(|p| p.default_speakers.clone()).unwrap_or_default()
    });

    let exam_format = state().format.format();
    let part_spec = exam_format.part(state().part).cloned();
    let file_prefix = format!("{}_Part{}", state().format.key().to_uppercase(), state().part);

    let handle_generate_topic = move |_| {
        state.write().topic_error = None;
        state.write().is_generating_topic = true;
        let (format, part) = (state().format, state().part);
        spawn(async move {
            match suggest_topic(format, part, String::new()).await {
                Ok(topic) => state.write().topic = topic,
                Err(e) => state.write().topic_error = Some(format!("Could not suggest a topic: {e}")),
            }
            state.write().is_generating_topic = false;
        });
    };

    let handle_generate_script = move |_| {
        let request = PassageRequest {
            format: state().format,
            part: state().part,
            topic: state().topic.clone(),
            speakers: speakers(),
        };
        if let Err(e) = request.validate() {
            state.write().script_error = Some(e.to_string());
            return;
        }
        state.write().reset_results();
        state.write().is_generating_script = true;
        spawn(async move {
            match generate_passage(request).await {
                Ok(draft) => {
                    let mut s = state.write();
                    s.passage = Some(draft.passage);
                    s.passage_issues = draft.issues;
                }
                Err(e) => state.write().script_error = Some(format!("Script generation failed: {e}")),
            }
            state.write().is_generating_script = false;
        });
    };

    let handle_generate_tasks = move |_| {
        let Some(passage) = state().passage.clone() else { return };
        let (format, part) = (state().format, state().part);
        let task_count = format.format().part(part).map(|p| p.tasks.len()).unwrap_or(0);
        let current_speakers = speakers();
        {
            let mut s = state.write();
            s.tasks.clear();
            s.task_issues.clear();
            s.tasks_error = None;
            s.is_generating_tasks = true;
        }
        spawn(async move {
            for task_index in 0..task_count {
                let request = TaskRequest {
                    format,
                    part,
                    task_index,
                    passage: passage.clone(),
                    speakers: current_speakers.clone(),
                };
                match generate_task(request).await {
                    Ok(draft) => {
                        let mut s = state.write();
                        s.tasks.push(draft.task);
                        s.task_issues.extend(draft.issues);
                    }
                    Err(e) => {
                        state.write().tasks_error = Some(format!("Question generation failed: {e}"));
                        break;
                    }
                }
            }
            state.write().is_generating_tasks = false;
        });
    };

    let handle_generate_audio = move |_| {
        let Some(passage) = state().passage.clone() else { return };
        let request = AudioRequest { passage, speakers: speakers() };
        if let Err(e) = request.validate() {
            state.write().audio_error = Some(e.to_string());
            return;
        }
        state.write().is_generating_audio = true;
        state.write().audio_error = None;
        spawn(async move {
            let outcome = run_audio_job(request).await;
            let mut s = state.write();
            match outcome {
                Ok(bytes) => s.generated_audio = Some(bytes),
                Err(message) => s.audio_error = Some(message),
            }
            s.is_generating_audio = false;
        });
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/styling/generator.css") }

        div { class: "generator-container",
            div { class: "generator-header",
                h1 { "Listening Exam Generator" }
                p { "Script, questions, key and audio for one part at a time, in the format you choose." }
            }

            div { class: "generator-grid",
                // Column 1: format, part, topic
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Exam part" }

                    div { class: "config-group",
                        label { class: "input-label", "Format:" }
                        div { class: "select-wrapper",
                            select {
                                class: "section-select",
                                value: "{state().format.key()}",
                                onchange: move |evt| {
                                    if let Some(format) = FormatId::from_key(&evt.value()) {
                                        state.set(HomeState::for_format(format));
                                    }
                                },
                                for id in FormatId::ALL {
                                    option { value: "{id.key()}", "{id.format().name}" }
                                }
                            }
                        }
                    }

                    div { class: "config-group",
                        label { class: "input-label", "Part:" }
                        div { class: "select-wrapper",
                            select {
                                class: "section-select",
                                value: "{state().part}",
                                onchange: move |evt| {
                                    if let Ok(number) = evt.value().parse::<u8>() {
                                        let mut s = state.write();
                                        s.part = number;
                                        s.topic.clear();
                                        s.custom_speakers.clear();
                                        s.editing_speaker_idx = None;
                                        s.reset_results();
                                    }
                                },
                                for part in exam_format.parts.iter() {
                                    option { value: "{part.number}", "{part.title}: {part.passage.label()}" }
                                }
                            }
                        }
                    }

                    if let Some(spec) = part_spec.clone() {
                        div { class: "info-box",
                            p { class: "info-title", "{spec.title} - played {spec.playback.label()}" }
                            p { "{spec.brief}" }
                            p { "Length {spec.duration_label()}, questions {spec.first_item()} to {spec.last_item()}:" }
                            ul {
                                for task in spec.tasks.iter() {
                                    li { "{task.range_label()}: {task.kind.label()}" }
                                }
                            }
                        }
                    }

                    label { class: "input-label", "Topic or scenario:" }
                    div { class: "textarea-container",
                        textarea {
                            class: "input-textarea",
                            placeholder: "Example: A radio interview about how city councils are handling a heatwave.",
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
                            title: "Suggest a topic for this part",
                            if state().is_generating_topic { "Suggesting..." } else { "Suggest a topic" }
                        }
                    }
                    if let Some(error) = state().topic_error.clone() {
                        div { class: "error-message", "{error}" }
                    }
                }

                // Column 2: speakers
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Voices" }

                    div { class: "customize-section",
                        button {
                            class: "customize-toggle",
                            onclick: move |_| {
                                let current = state().show_speakers;
                                state.write().show_speakers = !current;
                            },
                            span { "Customize speakers" }
                            span { class: "toggle-icon", if state().show_speakers { "\u{25bc}" } else { "\u{25b6}" } }
                        }

                        if state().show_speakers {
                            div { class: "speakers-list",
                                for (idx, speaker) in speakers().iter().enumerate() {
                                    div { class: "speaker-card", key: "{idx}",
                                        div { class: "speaker-card-header",
                                            div { class: "speaker-name", "{speaker.label}" }
                                            button {
                                                class: "edit-button",
                                                onclick: move |_| state.write().editing_speaker_idx = Some(idx),
                                                "Edit"
                                            }
                                        }
                                        div { class: "speaker-details",
                                            span { class: "speaker-badge", "{speaker.gender.label()}" }
                                            span { class: "speaker-badge", "{speaker.accent.label()}" }
                                            span { class: "speaker-badge role", "{speaker.role.label()}" }
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(idx) = state().editing_speaker_idx {
                            if let Some(speaker) = speakers().get(idx) {
                                SpeakerEditModal {
                                    speaker: speaker.clone(),
                                    onclose: move |_| state.write().editing_speaker_idx = None,
                                    onsave: move |updated: SpeakerConfig| {
                                        let mut list = speakers();
                                        if idx < list.len() {
                                            list[idx] = updated;
                                        }
                                        let mut s = state.write();
                                        s.custom_speakers = list;
                                        s.editing_speaker_idx = None;
                                    }
                                }
                            }
                        }
                    }

                    div { class: "info-box",
                        p { class: "info-title", "How voices are used" }
                        p {
                            "Up to two voices are read in one pass. A part with three voices (host and two guests) \
                             is read turn by turn and joined, which takes longer."
                        }
                    }
                }

                // Column 3: results
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Results" }

                    if let Some(error) = state().script_error.clone() {
                        div { class: "error-box", span { class: "error-icon", "!" } span { "{error}" } }
                    }

                    if let Some(passage) = state().passage.clone() {
                        div { class: "script-result",
                            div { class: "success-banner",
                                span { class: "success-icon", "\u{2713}" }
                                span { "Script ready: {passage.word_count()} words, about {passage.estimated_minutes():.1} min" }
                            }

                            IssueList { issues: state().passage_issues.clone() }

                            div { class: "script-preview",
                                h3 { "Script" }
                                pre { class: "script-content", "{passage.script_text()}" }
                            }

                            div { class: "download-buttons",
                                button {
                                    class: "download-button primary",
                                    disabled: state().is_generating_tasks,
                                    onclick: handle_generate_tasks,
                                    if state().is_generating_tasks { "Writing questions..." } else { "Generate questions" }
                                }
                                button {
                                    class: "download-button secondary",
                                    disabled: state().is_generating_audio,
                                    onclick: handle_generate_audio,
                                    if state().is_generating_audio { "Recording..." } else { "Generate audio" }
                                }
                                button {
                                    class: "download-button secondary",
                                    onclick: {
                                        let script = passage.script_text();
                                        let name = format!("{file_prefix}_script.txt");
                                        move |_| download_text(&script, &name)
                                    },
                                    "Download script"
                                }
                            }

                            if let Some(error) = state().tasks_error.clone() {
                                div { class: "error-box", span { class: "error-icon", "!" } span { "{error}" } }
                            }

                            if !state().tasks.is_empty() {
                                if let Some(spec) = part_spec.clone() {
                                    {
                                        let tasks = state().tasks.clone();
                                        let paper = markdown::render_part_paper(&spec, &tasks);
                                        let key = markdown::render_key(&tasks);
                                        let transcript = markdown::render_transcript(&passage, &speakers());
                                        let document = format!("{paper}\n### Key\n\n{key}\n{transcript}");
                                        let name = format!("{file_prefix}_paper.md");
                                        rsx! {
                                            IssueList { issues: state().task_issues.clone() }
                                            div { class: "script-preview",
                                                h3 { "Questions" }
                                                pre { class: "script-content", "{paper}" }
                                                h3 { "Key" }
                                                pre { class: "script-content", "{key}" }
                                            }
                                            div { class: "download-buttons",
                                                button {
                                                    class: "download-button primary",
                                                    onclick: move |_| download_text(&document, &name),
                                                    "Download paper + key (Markdown)"
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(error) = state().audio_error.clone() {
                                div { class: "error-box", span { class: "error-icon", "!" } span { "{error}" } }
                            }

                            if let Some(audio_data) = state().generated_audio.clone() {
                                AudioPlayerSection {
                                    audio_data: audio_data,
                                    file_name: format!("{file_prefix}_audio.wav"),
                                }
                            }
                        }
                    } else if !state().is_generating_script && state().script_error.is_none() {
                        div { class: "empty-state",
                            p { "Choose a part, describe the topic and generate the script. Questions and audio follow from it." }
                        }
                    }
                }
            }

            div { class: "generate-button-container",
                button {
                    class: "generate-button",
                    disabled: state().is_generating_script,
                    onclick: handle_generate_script,
                    if state().is_generating_script { "Generating..." } else { "Generate script" }
                }
            }

            if state().is_generating_topic {
                LoadingPopup {
                    message: "Suggesting a topic...".to_string(),
                    submessage: "A few seconds".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_topic = false;
                        state.write().topic_error = Some("Cancelled".to_string());
                    }
                }
            }
            if state().is_generating_script {
                LoadingPopup {
                    message: "Writing the script...".to_string(),
                    submessage: "Usually 30-60 seconds".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_script = false;
                        state.write().script_error = Some("Cancelled".to_string());
                    }
                }
            }
            if state().is_generating_tasks {
                LoadingPopup {
                    message: "Writing the questions...".to_string(),
                    submessage: "One block at a time; each takes 20-40 seconds".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_tasks = false;
                        state.write().tasks_error = Some("Cancelled".to_string());
                    }
                }
            }
            if state().is_generating_audio {
                LoadingPopup {
                    message: "Recording the audio...".to_string(),
                    submessage: "Two to five minutes for a long part".to_string(),
                    oncancel: move |_| {
                        state.write().is_generating_audio = false;
                        state.write().audio_error = Some("Cancelled".to_string());
                    }
                }
            }
        }
    }
}

/// Validator findings, one line each.
#[component]
fn IssueList(issues: Vec<ValidationIssue>) -> Element {
    if issues.is_empty() {
        return rsx! {};
    }
    rsx! {
        ul { class: "issue-list",
            for issue in issues.iter() {
                li { class: "issue-item", "{issue.display()}" }
            }
        }
    }
}

/// Starts a synthesis job and polls it to completion.
async fn run_audio_job(request: AudioRequest) -> Result<Vec<u8>, String> {
    let job_id = start_part_audio(request).await.map_err(|e| format!("Could not start the recording: {e}"))?;
    for _ in 0..MAX_POLLS {
        sleep_ms(POLL_INTERVAL_MS).await;
        let job = audio_job_status(job_id.clone()).await.map_err(|e| format!("Could not check the recording: {e}"))?;
        match job.status {
            JobStatus::Completed => {
                return audio_job_result(job_id).await.map_err(|e| format!("Could not fetch the recording: {e}"));
            }
            JobStatus::Failed => return Err(job.error.unwrap_or_else(|| "The recording failed".to_string())),
            JobStatus::Pending | JobStatus::Processing => {}
        }
    }
    Err("The recording timed out after five minutes. Please try again.".to_string())
}

#[cfg(target_arch = "wasm32")]
async fn sleep_ms(ms: u32) {
    TimeoutFuture::new(ms).await;
}

#[cfg(not(target_arch = "wasm32"))]
async fn sleep_ms(ms: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}
