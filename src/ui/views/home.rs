//! Home view: choose a format and a part, then generate the script, the
//! questions and the audio, in one go or one piece at a time.
//!
//! The view holds one `HomeState` signal. Every server call goes through
//! `crate::application`; every rule check is already done by the domain, the
//! view only displays issues. The one-click pipeline runs in the browser by
//! chaining the same server functions the individual buttons use: the script
//! first, then the questions and the recording side by side.

use dioxus::prelude::*;

use crate::application::audio::{audio_url, start_part_audio};
use crate::application::passages::generate_passage;
use crate::application::tasks::generate_task;
use crate::application::topics::suggest_topic;
use crate::domain::{
    AudioRequest, AudioTrack, FormatId, Passage, PassageRequest, SpeakerConfig, Task, TaskRequest,
    ValidationIssue, has_errors,
};
use crate::export::{docx, markdown};
use crate::ui::components::audio_player::{AudioPlayerSection, download_bytes, download_text};
use crate::ui::components::issue_list::IssueList;
use crate::ui::components::loading_popup::LoadingPopup;
use crate::ui::components::speaker_modal::SpeakerEditModal;
use crate::ui::jobs::{PART_AUDIO_DEADLINE_MS, PART_AUDIO_POLL_MS, wait_for_job};

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

    /// Bumped whenever results are reset or a run is cancelled. A running
    /// pipeline compares against it before every write, so late results of a
    /// superseded run are dropped instead of overwriting the new ones.
    pub run: u32,
    /// Why the one-click pipeline stopped early, shown above the results.
    pub pipeline_note: Option<String>,

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
    /// A recording job started on the server whose result is not here yet
    /// (still running, timed out or cancelled locally); "Check again" fetches it.
    pub audio_job_id: Option<String>,
    /// The finished recording: the server streams it at `audio_url`.
    pub audio: Option<AudioTrack>,
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
            run: 0,
            pipeline_note: None,
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
            audio_job_id: None,
            audio: None,
        }
    }

    /// A fresh state for another format that still supersedes runs in flight.
    fn switch_format(&self, format: FormatId) -> Self {
        Self {
            run: self.run + 1,
            ..Self::for_format(format)
        }
    }

    /// Everything derived from the current part, kept; the rest cleared.
    /// Also supersedes any run still in flight.
    fn reset_results(&mut self) {
        self.run += 1;
        self.pipeline_note = None;
        self.is_generating_script = false;
        self.script_error = None;
        self.passage = None;
        self.passage_issues.clear();
        self.is_generating_tasks = false;
        self.tasks_error = None;
        self.tasks.clear();
        self.task_issues.clear();
        self.is_generating_audio = false;
        self.audio_error = None;
        self.audio_job_id = None;
        self.audio = None;
    }

    fn is_busy(&self) -> bool {
        self.is_generating_topic
            || self.is_generating_script
            || self.is_generating_tasks
            || self.is_generating_audio
    }

    fn task_count(&self) -> usize {
        self.format
            .format()
            .part(self.part)
            .map(|p| p.tasks.len())
            .unwrap_or(0)
    }

    /// What the single progress popup says while something is running.
    fn busy_message(&self) -> Option<(String, String)> {
        let (message, detail) = if self.is_generating_script {
            (
                "Writing the script...".to_string(),
                "Usually 30-60 seconds".to_string(),
            )
        } else if self.is_generating_tasks && self.is_generating_audio {
            (
                format!(
                    "Writing questions ({} of {}) and recording the audio...",
                    self.tasks.len(),
                    self.task_count()
                ),
                "Two to ten minutes; the script is already readable below".to_string(),
            )
        } else if self.is_generating_tasks {
            (
                format!(
                    "Writing the questions ({} of {})...",
                    self.tasks.len(),
                    self.task_count()
                ),
                "One block at a time; each takes 20-40 seconds".to_string(),
            )
        } else if self.is_generating_audio {
            (
                "Recording the audio...".to_string(),
                "Two to five minutes; up to ten with three voices".to_string(),
            )
        } else if self.is_generating_topic {
            (
                "Suggesting a topic...".to_string(),
                "A few seconds".to_string(),
            )
        } else {
            return None;
        };
        Some((message, detail))
    }

    /// Stops listening to whatever is running. Work already started on the
    /// server is not interrupted; a recording keeps its job id for "Check again".
    fn cancel(&mut self) {
        self.run += 1;
        if self.is_generating_topic {
            self.is_generating_topic = false;
            self.topic_error = Some("Cancelled".into());
        }
        if self.is_generating_script {
            self.is_generating_script = false;
            self.script_error = Some("Cancelled".into());
        }
        if self.is_generating_tasks {
            self.is_generating_tasks = false;
            self.tasks_error = Some("Cancelled".into());
        }
        if self.is_generating_audio {
            self.is_generating_audio = false;
            self.audio_error = Some(if self.audio_job_id.is_some() {
                "Cancelled here; the recording keeps running on the server. Use \"Check again\" to fetch it.".into()
            } else {
                "Cancelled".into()
            });
        }
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
        current
            .format
            .format()
            .part(current.part)
            .map(|p| p.default_speakers.clone())
            .unwrap_or_default()
    });

    let exam_format = state().format.format();
    let part_spec = exam_format.part(state().part).cloned();
    let file_prefix = format!(
        "{}_Part{}",
        state().format.key().to_uppercase(),
        state().part
    );

    let handle_generate_topic = move |_| {
        state.write().topic_error = None;
        state.write().is_generating_topic = true;
        let (format, part, run) = (state().format, state().part, state().run);
        spawn(async move {
            let outcome = suggest_topic(format, part, String::new()).await;
            if !still_current(state, run) {
                return;
            }
            let mut s = state.write();
            match outcome {
                Ok(topic) => s.topic = topic,
                Err(e) => s.topic_error = Some(format!("Could not suggest a topic: {e}")),
            }
            s.is_generating_topic = false;
        });
    };

    let handle_generate_script = move |_| {
        let request = match build_script_request(&state(), speakers()) {
            Ok(request) => request,
            Err(message) => {
                state.write().script_error = Some(message);
                return;
            }
        };
        state.write().reset_results();
        let run = state().run;
        spawn(async move {
            run_script(state, run, request).await;
        });
    };

    let handle_generate_tasks = move |_| {
        let Some(passage) = state().passage.clone() else {
            return;
        };
        let (format, part, run) = (state().format, state().part, state().run);
        spawn(run_tasks(state, run, format, part, passage, speakers()));
    };

    let handle_generate_audio = move |_| {
        let Some(passage) = state().passage.clone() else {
            return;
        };
        let request = AudioRequest {
            passage,
            speakers: speakers(),
        };
        if let Err(e) = request.validate() {
            state.write().audio_error = Some(e.to_string());
            return;
        }
        let run = state().run;
        spawn(run_audio(state, run, request));
    };

    let handle_check_audio = move |_| {
        let Some(job_id) = state().audio_job_id.clone() else {
            return;
        };
        state.write().audio_error = None;
        let run = state().run;
        spawn(fetch_audio(state, run, job_id));
    };

    // The one-click pipeline: script, then questions and recording side by side.
    let handle_generate_all = move |_| {
        let request = match build_script_request(&state(), speakers()) {
            Ok(request) => request,
            Err(message) => {
                state.write().script_error = Some(message);
                return;
            }
        };
        state.write().reset_results();
        let run = state().run;
        let (format, part) = (request.format, request.part);
        let current_speakers = request.speakers.clone();
        spawn(async move {
            let Some(passage) = run_script(state, run, request).await else {
                return;
            };
            let script_ok = !has_errors(&state.peek().passage_issues);
            if !script_ok {
                state.write().pipeline_note =
                    Some("The script has errors; fix or regenerate it before the questions and the audio.".into());
                return;
            }
            let audio_request = AudioRequest {
                passage: passage.clone(),
                speakers: current_speakers.clone(),
            };
            let audio_request = match audio_request.validate() {
                Ok(()) => Some(audio_request),
                Err(e) => {
                    state.write().audio_error = Some(e.to_string());
                    None
                }
            };
            let tasks = run_tasks(state, run, format, part, passage, current_speakers);
            match audio_request {
                Some(audio_request) => {
                    futures_util::future::join(tasks, run_audio(state, run, audio_request)).await;
                }
                None => tasks.await,
            }
        });
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/styling/generator.css") }

        div { class: "generator-container",
            div { class: "generator-header",
                h1 { "Listening Exam Generator" }
                p { "Script, questions, key, transcript and audio for one part, in one go or step by step." }
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
                                        let fresh = state().switch_format(format);
                                        state.set(fresh);
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

                    if let Some(note) = state().pipeline_note.clone() {
                        div { class: "note-box", span { class: "note-icon", "!" } span { "{note}" } }
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
                                button {
                                    class: "download-button info",
                                    onclick: {
                                        let transcript = markdown::render_transcript(&passage, &speakers());
                                        let name = format!("{file_prefix}_transcript.md");
                                        move |_| download_text(&transcript, &name)
                                    },
                                    "Download transcript (Markdown)"
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
                                        let docx_spec = spec.clone();
                                        let docx_name = format!("{file_prefix}_paper.docx");
                                        let markdown_spec = spec.clone();
                                        let markdown_name = format!("{file_prefix}_paper.md");
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
                                                    onclick: move |_| {
                                                        let s = state.peek();
                                                        let voices = speakers.peek();
                                                        let transcript = s.passage.as_ref().map(|p| (p, voices.as_slice()));
                                                        let bytes = docx::render_part_docx(&docx_spec, &s.tasks, transcript);
                                                        download_bytes(&bytes, docx::DOCX_MIME, &docx_name);
                                                    },
                                                    "Download paper + key + transcript (DOCX)"
                                                }
                                                button {
                                                    class: "download-button info",
                                                    onclick: move |_| {
                                                        let s = state.peek();
                                                        let paper = markdown::render_part_paper(&markdown_spec, &s.tasks);
                                                        let key = markdown::render_key(&s.tasks);
                                                        let transcript = s
                                                            .passage
                                                            .as_ref()
                                                            .map(|p| markdown::render_transcript(p, &speakers.peek()))
                                                            .unwrap_or_default();
                                                        download_text(&format!("{paper}\n### Key\n\n{key}\n{transcript}"), &markdown_name);
                                                    },
                                                    "Download paper + key + transcript (Markdown)"
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(error) = state().audio_error.clone() {
                                div { class: "error-box", span { class: "error-icon", "!" } span { "{error}" } }
                            }

                            if state().audio_job_id.is_some() && !state().is_generating_audio && state().audio.is_none() {
                                div { class: "download-buttons",
                                    button {
                                        class: "download-button info",
                                        onclick: handle_check_audio,
                                        "Check again"
                                    }
                                }
                            }

                            if let Some(track) = state().audio.clone() {
                                AudioPlayerSection {
                                    src: audio_url(&track.location),
                                    file_name: format!("{file_prefix}_audio.wav"),
                                    duration_ms: Some(track.duration_ms),
                                }
                            }
                        }
                    } else if !state().is_generating_script && state().script_error.is_none() {
                        div { class: "empty-state",
                            p { "Choose a part, describe the topic and generate. Questions and audio follow from the script, together or step by step." }
                        }
                    }
                }
            }

            div { class: "generate-button-container",
                button {
                    class: "generate-button",
                    disabled: state().is_busy(),
                    onclick: handle_generate_all,
                    if state().is_busy() { "Working..." } else { "Generate script, questions and audio" }
                }
                button {
                    class: "generate-button secondary",
                    disabled: state().is_busy(),
                    onclick: handle_generate_script,
                    "Script only"
                }
            }

            if let Some((message, submessage)) = state().busy_message() {
                LoadingPopup {
                    message,
                    submessage,
                    oncancel: move |_| state.write().cancel(),
                }
            }
        }
    }
}

/// Validates the form before a script request leaves the browser.
fn build_script_request(
    state: &HomeState,
    speakers: Vec<SpeakerConfig>,
) -> Result<PassageRequest, String> {
    let request = PassageRequest {
        format: state.format,
        part: state.part,
        topic: state.topic.clone(),
        speakers,
    };
    request.validate().map_err(|e| e.to_string())?;
    Ok(request)
}

/// True while `run` is still the run whose results the state expects.
fn still_current(state: Signal<HomeState>, run: u32) -> bool {
    state.peek().run == run
}

/// Generates the script and stores the draft. Returns the passage so a
/// pipeline can carry on with it.
async fn run_script(
    mut state: Signal<HomeState>,
    run: u32,
    request: PassageRequest,
) -> Option<Passage> {
    state.write().is_generating_script = true;
    let outcome = generate_passage(request).await;
    if !still_current(state, run) {
        return None;
    }
    let mut s = state.write();
    s.is_generating_script = false;
    match outcome {
        Ok(draft) => {
            s.passage = Some(draft.passage.clone());
            s.passage_issues = draft.issues;
            Some(draft.passage)
        }
        Err(e) => {
            s.script_error = Some(format!("Script generation failed: {e}"));
            None
        }
    }
}

/// Generates every task block of the part, one after another, appending each
/// draft as it arrives.
async fn run_tasks(
    mut state: Signal<HomeState>,
    run: u32,
    format: FormatId,
    part: u8,
    passage: Passage,
    speakers: Vec<SpeakerConfig>,
) {
    let task_count = format
        .format()
        .part(part)
        .map(|p| p.tasks.len())
        .unwrap_or(0);
    {
        let mut s = state.write();
        s.tasks.clear();
        s.task_issues.clear();
        s.tasks_error = None;
        s.is_generating_tasks = true;
    }
    for task_index in 0..task_count {
        let request = TaskRequest {
            format,
            part,
            task_index,
            passage: passage.clone(),
            speakers: speakers.clone(),
        };
        let outcome = generate_task(request).await;
        if !still_current(state, run) {
            return;
        }
        match outcome {
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
}

/// Starts the recording job and waits for its WAV.
async fn run_audio(mut state: Signal<HomeState>, run: u32, request: AudioRequest) {
    {
        let mut s = state.write();
        s.is_generating_audio = true;
        s.audio_error = None;
        s.audio_job_id = None;
        s.audio = None;
    }
    let started = start_part_audio(request).await;
    if !still_current(state, run) {
        return;
    }
    let job_id = match started {
        Ok(job_id) => job_id,
        Err(e) => {
            let mut s = state.write();
            s.audio_error = Some(format!("Could not start the recording: {e}"));
            s.is_generating_audio = false;
            return;
        }
    };
    state.write().audio_job_id = Some(job_id.clone());
    fetch_audio(state, run, job_id).await;
}

/// Waits for a started job and records where its WAV is; also behind "Check again".
async fn fetch_audio(mut state: Signal<HomeState>, run: u32, job_id: String) {
    state.write().is_generating_audio = true;
    let outcome = wait_for_job(&job_id, PART_AUDIO_POLL_MS, PART_AUDIO_DEADLINE_MS, |_| {}).await;
    if !still_current(state, run) {
        return;
    }
    let mut s = state.write();
    match outcome {
        Ok(job) => match job.track {
            Some(track) => {
                s.audio = Some(track);
                s.audio_job_id = None;
            }
            None => s.audio_error = Some("The recording finished but its file is missing".into()),
        },
        Err(message) => s.audio_error = Some(message),
    }
    s.is_generating_audio = false;
}
