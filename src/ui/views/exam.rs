//! Exam view: every part of a format in one go, with one recording.
//!
//! The teacher gives each part a topic (or asks for suggestions), sets a
//! title and presses one button. The browser then runs every part's script
//! in parallel, and afterwards every part's questions in parallel alongside
//! one `start_exam_audio` job that renders the format's `AudioProgram`
//! (music, tones, announcements, reading pauses, replays). The `domain::Exam`
//! in the state is the source of truth for what was generated; `PartWork` and
//! `AudioWork` only hold what is running and what went wrong. The state is
//! provided by the `Navbar` layout, so moving between pages keeps a running
//! exam, and pipelines are spawned on the root scope for the same reason.

use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use futures_util::future::{join, join_all};

use crate::application::audio::{audio_url, start_exam_audio};
use crate::application::passages::generate_passage;
use crate::application::tasks::generate_task;
use crate::application::topics::suggest_topic;
use crate::domain::{
    AudioRequest, AudioTrack, Exam, ExamAudioRequest, FormatId, PassageRequest, TaskRequest,
    ValidationIssue, has_errors, validate_exam,
};
use crate::export::markdown;
use crate::ui::components::audio_player::{AudioPlayerSection, download_text};
use crate::ui::components::issue_list::IssueList;
use crate::ui::components::loading_popup::LoadingPopup;
use crate::ui::jobs::{EXAM_AUDIO_DEADLINE_MS, EXAM_AUDIO_POLL_MS, wait_for_job};

/// Where one piece of work stands.
#[derive(Clone, PartialEq, Default)]
pub enum Step {
    #[default]
    Idle,
    Running,
    Done,
    Failed(String),
}

impl Step {
    fn is_running(&self) -> bool {
        matches!(self, Step::Running)
    }
}

/// Transient state of one part: what the teacher typed and what is running.
#[derive(Clone, Default)]
pub struct PartWork {
    pub topic: String,
    pub topic_step: Step,
    pub script_step: Step,
    pub tasks_step: Step,
    pub passage_issues: Vec<ValidationIssue>,
    pub task_issues: Vec<ValidationIssue>,
}

/// The whole-exam recording job.
#[derive(Clone, Default)]
pub struct AudioWork {
    pub step: Step,
    pub job_id: Option<String>,
    pub progress: f32,
    pub track: Option<AudioTrack>,
    /// A script was regenerated after this recording was made.
    pub stale: bool,
}

#[derive(Clone)]
pub struct ExamState {
    pub format: FormatId,
    pub exam: Exam,
    /// Index-aligned with `exam.parts`.
    pub work: Vec<PartWork>,
    pub audio: AudioWork,
    /// Bumped on every new exam run or cancel; a running pipeline compares
    /// against it before every write, so late results of an old run are dropped.
    pub run: u32,
}

impl Default for ExamState {
    fn default() -> Self {
        Self::for_format(FormatId::HsgNational)
    }
}

impl ExamState {
    fn for_format(format: FormatId) -> Self {
        let exam_format = format.format();
        let title = format!("{} - draft", exam_format.name);
        let exam = Exam::new(exam_format, title, "");
        let work = exam.parts.iter().map(|_| PartWork::default()).collect();
        Self {
            format,
            exam,
            work,
            audio: AudioWork::default(),
            run: 0,
        }
    }

    /// A fresh exam in another format that still supersedes runs in flight.
    fn switch_format(&self, format: FormatId) -> Self {
        Self {
            run: self.run + 1,
            ..Self::for_format(format)
        }
    }

    fn is_busy(&self) -> bool {
        self.audio.step.is_running()
            || self.work.iter().any(|w| {
                w.topic_step.is_running() || w.script_step.is_running() || w.tasks_step.is_running()
            })
    }

    fn scripts_done(&self) -> usize {
        self.exam
            .parts
            .iter()
            .filter(|p| p.passage.is_some())
            .count()
    }

    /// What the single progress popup says while something is running.
    fn busy_message(&self) -> Option<(String, String)> {
        let n = self.exam.parts.len();
        let scripts_running = self
            .work
            .iter()
            .filter(|w| w.script_step.is_running())
            .count();
        let tasks_running = self
            .work
            .iter()
            .filter(|w| w.tasks_step.is_running())
            .count();
        let topics_running = self
            .work
            .iter()
            .filter(|w| w.topic_step.is_running())
            .count();
        let audio_running = self.audio.step.is_running();
        let (message, detail) = if scripts_running > 0 {
            (
                format!("Writing scripts ({} of {} done)...", n - scripts_running, n),
                "All parts at once; usually about a minute".to_string(),
            )
        } else if tasks_running > 0 && audio_running {
            (
                format!(
                    "Writing questions ({tasks_running} part(s) left) and recording the exam: {}...",
                    audio_stage(self.audio.progress, n)
                ),
                "Questions take a few minutes; the recording ten to thirty".to_string(),
            )
        } else if tasks_running > 0 {
            (
                format!("Writing questions ({tasks_running} part(s) left)..."),
                "One block at a time per part".to_string(),
            )
        } else if audio_running {
            (
                format!(
                    "Recording the exam: {}...",
                    audio_stage(self.audio.progress, n)
                ),
                "Ten to thirty minutes; you can visit the other page and come back".to_string(),
            )
        } else if topics_running > 0 {
            (
                "Suggesting topics...".to_string(),
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
        for work in &mut self.work {
            for step in [
                &mut work.topic_step,
                &mut work.script_step,
                &mut work.tasks_step,
            ] {
                if step.is_running() {
                    *step = Step::Failed("Cancelled".into());
                }
            }
        }
        if self.audio.step.is_running() {
            self.audio.step = Step::Failed(if self.audio.job_id.is_some() {
                "Cancelled here; the recording keeps running on the server. Use \"Check again\" to fetch it.".into()
            } else {
                "Cancelled".into()
            });
        }
    }
}

/// What the exam job is doing, read off the progress the worker reports:
/// 0.1 at start, then 0.1 + 0.7 x (parts done / parts), then assembly to 1.0.
fn audio_stage(progress: f32, parts: usize) -> String {
    if parts == 0 || progress >= 0.8 {
        return "assembling announcements, pauses and replays".into();
    }
    let done = (((progress - 0.1) / 0.7) * parts as f32).round().max(0.0) as usize;
    format!("reading part {} of {}", (done + 1).min(parts), parts)
}

#[component]
pub fn ExamView() -> Element {
    let mut state = use_context::<Signal<ExamState>>();

    let current = state();
    let busy = current.is_busy();
    let part_count = current.exam.parts.len();
    let exam_issues = validate_exam(&current.exam);
    let has_any_script = current.scripts_done() > 0;
    let all_scripts = current.scripts_done() == part_count;
    let id8: String = current.exam.id.to_string().chars().take(8).collect();
    let file_prefix = format!("{}_exam_{}", current.format.key().to_uppercase(), id8);
    let audio_pct = format!("{:.0}", (current.audio.progress * 100.0).clamp(0.0, 100.0));

    // The one-click pipeline: topics for parts that have none, then every
    // script at once, then every part's questions at once beside the single
    // exam recording job.
    let handle_generate_exam = move |_| {
        let run = {
            let mut s = state.write();
            s.run += 1;
            s.audio = AudioWork::default();
            s.run
        };
        let n = state.peek().exam.parts.len();
        let untitled = empty_topics(&state.peek());
        spawn_forever(async move {
            join_all(
                untitled
                    .into_iter()
                    .map(|i| suggest_part_topic(state, run, i)),
            )
            .await;
            if !still_current(state, run) {
                return;
            }
            let ready = join_all((0..n).map(|i| run_part_script(state, run, i))).await;
            if !still_current(state, run) {
                return;
            }
            let task_runs = join_all(
                (0..n)
                    .filter(|i| ready[*i])
                    .map(|i| run_part_tasks(state, run, i)),
            );
            if ready.iter().all(|ok| *ok) {
                let request = {
                    let s = state.peek();
                    exam_audio_request(&s)
                };
                match request {
                    Ok(request) => {
                        join(task_runs, run_exam_audio(state, run, request)).await;
                    }
                    Err(message) => {
                        state.write().audio.step = Step::Failed(message);
                        task_runs.await;
                    }
                }
            } else {
                state.write().audio.step = Step::Failed(
                    "Fix the scripts that failed or have errors, then render the recording.".into(),
                );
                task_runs.await;
            }
        });
    };

    let handle_suggest_all = move |_| {
        let run = state.peek().run;
        let targets = empty_topics(&state.peek());
        spawn_forever(async move {
            join_all(
                targets
                    .into_iter()
                    .map(|i| suggest_part_topic(state, run, i)),
            )
            .await;
        });
    };

    let handle_render_audio = move |_| {
        let request = {
            let s = state.peek();
            exam_audio_request(&s)
        };
        match request {
            Ok(request) => {
                let run = state.peek().run;
                spawn_forever(run_exam_audio(state, run, request));
            }
            Err(message) => state.write().audio.step = Step::Failed(message),
        }
    };

    let handle_check_audio = move |_| {
        let Some(job_id) = state.peek().audio.job_id.clone() else {
            return;
        };
        let run = state.peek().run;
        spawn_forever(fetch_exam_audio(state, run, job_id));
    };

    let audio_body = match current.audio.step.clone() {
        Step::Idle => rsx! {
            p { class: "muted", "Made together with the exam, or on demand once every part has a script." }
        },
        Step::Running => rsx! {
            div { class: "progress-bar", div { class: "progress-fill", style: "width: {audio_pct}%" } }
            p { class: "muted", "Recording: {audio_stage(current.audio.progress, part_count)}" }
        },
        Step::Done => rsx! {},
        Step::Failed(message) => rsx! {
            div { class: "error-box", span { class: "error-icon", "!" } span { "{message}" } }
        },
    };

    rsx! {
        document::Stylesheet { href: asset!("/assets/styling/generator.css") }

        div { class: "generator-container",
            div { class: "generator-header",
                h1 { "Whole exam" }
                p { "Every part's script, questions and key, the transcripts and one exam recording, from one button." }
            }

            div { class: "generator-panel exam-setup",
                div { class: "exam-setup-grid",
                    div { class: "config-group",
                        label { class: "input-label", "Format:" }
                        div { class: "select-wrapper",
                            select {
                                class: "section-select",
                                value: "{current.format.key()}",
                                onchange: move |evt| {
                                    if let Some(format) = FormatId::from_key(&evt.value()) {
                                        let fresh = state.peek().switch_format(format);
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
                        label { class: "input-label", "Title:" }
                        input {
                            class: "form-input",
                            value: "{current.exam.title}",
                            oninput: move |evt| state.write().exam.title = evt.value(),
                        }
                    }
                }
                label { class: "input-label", "Theme for topic suggestions (optional):" }
                textarea {
                    class: "input-textarea small",
                    placeholder: "Example: news listening about cities and the environment",
                    value: "{current.exam.theme}",
                    oninput: move |evt| state.write().exam.theme = evt.value(),
                }
                div { class: "download-buttons",
                    button {
                        class: "download-button info",
                        disabled: busy,
                        onclick: handle_suggest_all,
                        "Suggest topics for empty parts"
                    }
                    button {
                        class: "download-button secondary",
                        disabled: busy || !all_scripts,
                        onclick: handle_render_audio,
                        "Render exam audio"
                    }
                }
            }

            div { class: "exam-parts",
                for (i, part) in current.exam.parts.iter().enumerate() {
                    {
                        let work = current.work[i].clone();
                        let spec = part.spec.clone();
                        let passage = part.passage.clone();
                        let tasks = part.tasks.clone();
                        let paper = markdown::render_part_paper(&spec, &tasks);
                        let key = markdown::render_key(&tasks);
                        let has_passage = passage.is_some();
                        rsx! {
                            div { class: "generator-panel part-card", key: "{spec.number}",
                                h3 { class: "part-title", "{spec.title}: {spec.passage.label()}" }
                                p { class: "part-meta",
                                    "Played {spec.playback.label()}, {spec.duration_label()}, questions {spec.first_item()} to {spec.last_item()}"
                                }
                                textarea {
                                    class: "input-textarea small",
                                    placeholder: "Topic or scenario for this part",
                                    value: "{work.topic}",
                                    oninput: move |evt| state.write().work[i].topic = evt.value(),
                                }
                                if let Step::Failed(message) = work.topic_step.clone() {
                                    div { class: "error-message", "{message}" }
                                }
                                div { class: "download-buttons",
                                    button {
                                        class: "download-button info",
                                        disabled: busy,
                                        onclick: move |_| {
                                            let run = state.peek().run;
                                            spawn_forever(suggest_part_topic(state, run, i));
                                        },
                                        if work.topic_step.is_running() { "Suggesting..." } else { "Suggest a topic" }
                                    }
                                    button {
                                        class: "download-button primary",
                                        disabled: busy,
                                        onclick: move |_| {
                                            let run = state.peek().run;
                                            spawn_forever(async move {
                                                if run_part_script(state, run, i).await {
                                                    run_part_tasks(state, run, i).await;
                                                }
                                            });
                                        },
                                        if has_passage { "Regenerate script + questions" } else { "Generate script + questions" }
                                    }
                                    if has_passage {
                                        button {
                                            class: "download-button secondary",
                                            disabled: busy,
                                            onclick: move |_| {
                                                let run = state.peek().run;
                                                spawn_forever(run_part_tasks(state, run, i));
                                            },
                                            "Regenerate questions"
                                        }
                                    }
                                }
                                StepLine { label: "Script".to_string(), step: work.script_step.clone() }
                                StepLine { label: "Questions".to_string(), step: work.tasks_step.clone() }
                                IssueList { issues: work.passage_issues.clone() }
                                IssueList { issues: work.task_issues.clone() }
                                if let Some(passage) = passage.clone() {
                                    details { class: "preview",
                                        summary { "Script: {passage.word_count()} words, about {passage.estimated_minutes():.1} min" }
                                        pre { class: "script-content", "{passage.script_text()}" }
                                    }
                                }
                                if !tasks.is_empty() {
                                    details { class: "preview",
                                        summary { "Questions and key ({tasks.len()} of {spec.tasks.len()} blocks)" }
                                        pre { class: "script-content", "{paper}" }
                                        pre { class: "script-content", "{key}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "exam-panels",
                div { class: "generator-panel",
                    h2 { class: "panel-header", "Exam recording" }
                    p { class: "panel-help",
                        "One WAV: music, announcements, a sound before each part, 20 seconds to read the questions, \
                         a replay for parts played twice, and the checking time at the end."
                    }
                    if current.audio.stale {
                        div { class: "note-box",
                            span { class: "note-icon", "!" }
                            span { "A script changed after this recording was made; render it again." }
                        }
                    }
                    {audio_body}
                    if let Some(track) = current.audio.track.clone() {
                        AudioPlayerSection {
                            src: audio_url(&track.location),
                            file_name: format!("{file_prefix}.wav"),
                            duration_ms: Some(track.duration_ms),
                        }
                    }
                    if current.audio.job_id.is_some() && !current.audio.step.is_running() && current.audio.track.is_none() {
                        div { class: "download-buttons",
                            button { class: "download-button info", onclick: handle_check_audio, "Check again" }
                        }
                    }
                }

                div { class: "generator-panel",
                    h2 { class: "panel-header", "Paper, key and transcripts" }
                    p { class: "panel-help",
                        "One Markdown document: every part's questions, the answer key and the transcripts."
                    }
                    IssueList { issues: exam_issues.clone() }
                    div { class: "download-buttons",
                        button {
                            class: "download-button primary",
                            disabled: !has_any_script,
                            onclick: {
                                let document = markdown::render_exam(&current.exam);
                                let name = format!("{file_prefix}.md");
                                move |_| download_text(&document, &name)
                            },
                            if exam_issues.is_empty() { "Download exam (Markdown)" } else { "Download draft (incomplete)" }
                        }
                    }
                }
            }

            div { class: "generate-button-container",
                button {
                    class: "generate-button",
                    disabled: busy,
                    onclick: handle_generate_exam,
                    if busy { "Working..." } else { "Generate the whole exam" }
                }
            }

            if let Some((message, submessage)) = current.busy_message() {
                LoadingPopup {
                    message,
                    submessage,
                    oncancel: move |_| state.write().cancel(),
                }
            }
        }
    }
}

/// One line of status for a piece of work.
#[component]
fn StepLine(label: String, step: Step) -> Element {
    let (class, text) = match &step {
        Step::Idle => ("step idle", "waiting".to_string()),
        Step::Running => ("step running", "in progress...".to_string()),
        Step::Done => ("step done", "done".to_string()),
        Step::Failed(message) => ("step failed", message.clone()),
    };
    rsx! {
        p { class: "{class}",
            span { class: "step-label", "{label}: " }
            span { "{text}" }
        }
    }
}

/// True while `run` is still the run whose results the state expects.
fn still_current(state: Signal<ExamState>, run: u32) -> bool {
    state.peek().run == run
}

/// Indices of the parts the teacher has not given a topic yet.
fn empty_topics(state: &ExamState) -> Vec<usize> {
    state
        .work
        .iter()
        .enumerate()
        .filter(|(_, w)| w.topic.trim().is_empty())
        .map(|(i, _)| i)
        .collect()
}

/// The script request of part `i`, validated in the browser first.
fn part_request(state: &ExamState, i: usize) -> Result<PassageRequest, String> {
    let part = &state.exam.parts[i];
    let request = PassageRequest {
        format: state.format,
        part: part.spec.number,
        topic: state.work[i].topic.clone(),
        speakers: part.speakers.clone(),
    };
    request.validate().map_err(|e| e.to_string())?;
    Ok(request)
}

/// Every part's passage as one `ExamAudioRequest`; the domain names the
/// first part that has no script yet.
fn exam_audio_request(state: &ExamState) -> Result<ExamAudioRequest, String> {
    let parts = state
        .exam
        .parts
        .iter()
        .filter_map(|p| {
            p.passage.clone().map(|passage| AudioRequest {
                passage,
                speakers: p.speakers.clone(),
            })
        })
        .collect();
    let request = ExamAudioRequest {
        format: state.format,
        parts,
    };
    request.validate().map_err(|e| e.to_string())?;
    Ok(request)
}

async fn suggest_part_topic(mut state: Signal<ExamState>, run: u32, i: usize) {
    let (format, number, theme) = {
        let s = state.peek();
        (s.format, s.exam.parts[i].spec.number, s.exam.theme.clone())
    };
    state.write().work[i].topic_step = Step::Running;
    let outcome = suggest_topic(format, number, theme).await;
    if !still_current(state, run) {
        return;
    }
    let mut s = state.write();
    match outcome {
        Ok(topic) => {
            s.work[i].topic = topic;
            s.work[i].topic_step = Step::Done;
        }
        Err(e) => s.work[i].topic_step = Step::Failed(format!("Could not suggest a topic: {e}")),
    }
}

/// Generates part `i`'s script. Returns true when it is usable for questions
/// and audio (present and without Error-severity issues).
async fn run_part_script(mut state: Signal<ExamState>, run: u32, i: usize) -> bool {
    let request = {
        let s = state.peek();
        part_request(&s, i)
    };
    let request = match request {
        Ok(request) => request,
        Err(message) => {
            state.write().work[i].script_step = Step::Failed(message);
            return false;
        }
    };
    {
        let mut s = state.write();
        s.exam.parts[i].passage = None;
        s.exam.parts[i].tasks.clear();
        s.work[i].script_step = Step::Running;
        s.work[i].passage_issues.clear();
        s.work[i].tasks_step = Step::Idle;
        s.work[i].task_issues.clear();
        if s.audio.track.is_some() {
            s.audio.stale = true;
        }
    }
    let outcome = generate_passage(request).await;
    if !still_current(state, run) {
        return false;
    }
    let mut s = state.write();
    match outcome {
        Ok(draft) => {
            let usable = !has_errors(&draft.issues);
            s.exam.parts[i].passage = Some(draft.passage);
            s.work[i].passage_issues = draft.issues;
            s.work[i].script_step = Step::Done;
            usable
        }
        Err(e) => {
            s.work[i].script_step = Step::Failed(format!("Script generation failed: {e}"));
            false
        }
    }
}

/// Generates every task block of part `i`, one after another.
async fn run_part_tasks(mut state: Signal<ExamState>, run: u32, i: usize) {
    let (format, number, passage, speakers, task_count) = {
        let s = state.peek();
        let part = &s.exam.parts[i];
        let Some(passage) = part.passage.clone() else {
            return;
        };
        (
            s.format,
            part.spec.number,
            passage,
            part.speakers.clone(),
            part.spec.tasks.len(),
        )
    };
    {
        let mut s = state.write();
        s.exam.parts[i].tasks.clear();
        s.work[i].task_issues.clear();
        s.work[i].tasks_step = Step::Running;
    }
    for task_index in 0..task_count {
        let request = TaskRequest {
            format,
            part: number,
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
                s.exam.parts[i].tasks.push(draft.task);
                s.work[i].task_issues.extend(draft.issues);
            }
            Err(e) => {
                state.write().work[i].tasks_step =
                    Step::Failed(format!("Question generation failed: {e}"));
                return;
            }
        }
    }
    state.write().work[i].tasks_step = Step::Done;
}

/// Starts the exam recording job and waits for it.
async fn run_exam_audio(mut state: Signal<ExamState>, run: u32, request: ExamAudioRequest) {
    state.write().audio = AudioWork {
        step: Step::Running,
        ..AudioWork::default()
    };
    let started = start_exam_audio(request).await;
    if !still_current(state, run) {
        return;
    }
    let job_id = match started {
        Ok(job_id) => job_id,
        Err(e) => {
            state.write().audio.step = Step::Failed(format!("Could not start the recording: {e}"));
            return;
        }
    };
    state.write().audio.job_id = Some(job_id.clone());
    fetch_exam_audio(state, run, job_id).await;
}

/// Waits for a started exam job, reporting progress; also behind "Check again".
async fn fetch_exam_audio(mut state: Signal<ExamState>, run: u32, job_id: String) {
    state.write().audio.step = Step::Running;
    let outcome = wait_for_job(
        &job_id,
        EXAM_AUDIO_POLL_MS,
        EXAM_AUDIO_DEADLINE_MS,
        move |progress| {
            if still_current(state, run) {
                state.write().audio.progress = progress;
            }
        },
    )
    .await;
    if !still_current(state, run) {
        return;
    }
    let mut s = state.write();
    match outcome {
        Ok(job) => match job.track {
            Some(track) => {
                s.audio.track = Some(track);
                s.audio.progress = 1.0;
                s.audio.stale = false;
                s.audio.step = Step::Done;
            }
            None => {
                s.audio.step = Step::Failed("The recording finished but its file is missing".into())
            }
        },
        Err(message) => s.audio.step = Step::Failed(message),
    }
}
