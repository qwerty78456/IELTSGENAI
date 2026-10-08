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
//!
//! The draft is saved on the server (`application::exams`): automatically
//! after every finished step once a script exists, a second after an edit,
//! and on the Save button. The layout-owned `SaveQueue` keeps per-exam
//! snapshots and single-flight requests alive across navigation. Opening a saved exam replaces the
//! state the way switching formats does, recomputing issues with the pure
//! validators and resuming a recording job that was still running.
//!
//! Speakers without a voice (a new exam, or one saved before 0.8) get one in
//! the browser once the voice catalogue is loaded (`assign_exam_voices`, as
//! the server does); `start_exam_audio` sends back the voices it recorded with,
//! kept as each part's `recorded_for`. Each part card edits its speakers
//! (`set_part_speakers`, the one place an edit lands). What an edit makes out
//! of date is derived, never flagged: a script whose `written_for` no longer
//! matches offers "Rewrite" or "Keep", and the recording is stale for a part
//! whose speakers sound different from its `recorded_for`.

use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use futures_util::future::{join, join_all};
use uuid::Uuid;

use crate::application::audio::{audio_url, start_exam_audio};
use crate::application::exams::{
    DeleteOutcome, ExamSummary, SaveOutcome, SavedExam, delete_exam, list_exams, load_exam,
    save_exam,
};
use crate::application::passages::generate_passage;
use crate::application::settings::{KeySource, api_key_status};
use crate::application::tasks::generate_task;
use crate::application::topics::suggest_topic;
use crate::application::usage::{UsageTotals, exam_usage, usage_totals};
use crate::domain::{
    AudioRequest, AudioTrack, Exam, ExamAudioRequest, ExamUsage, FormatId, PassageRequest,
    SpeakerConfig, TaskRequest, UsageStep, ValidationIssue, VoiceChoice, assign_exam_voices,
    assign_voices, has_errors, validate_exam, validate_passage, validate_task,
};
use crate::export::naming::test_type;
use crate::export::{docx, markdown};
use crate::ui::clock::local_time;
use crate::ui::components::audio_player::AudioPlayerSection;
use crate::ui::components::auto_download::AutoDownloadToggle;
use crate::ui::components::exam_library::ExamLibrary;
use crate::ui::components::issue_list::IssueList;
use crate::ui::components::loading_popup::LoadingPopup;
use crate::ui::components::speaker_modal::{SpeakerCards, SpeakerEditModal};
use crate::ui::components::voices::{
    VoiceCatalogueCtx, speaker_warnings, voices_of_others, voices_summary, with_choice,
};
use crate::ui::jobs::{EXAM_AUDIO_DEADLINE_MS, EXAM_AUDIO_POLL_MS, sleep_ms, wait_for_job};
use crate::ui::naming::{Content, DraftNaming, NameRequest, download, prefetch};
use crate::ui::prefs;
use crate::ui::save_queue::{SaveQueue, SaveStatus};

/// How long after the last keystroke an edit is saved.
const EDIT_DEBOUNCE_MS: u32 = 1_000;

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
    /// A script was regenerated after this recording was made. Speaker
    /// changes are not flagged here: they are derived per part
    /// (`ExamPart::recording_stale`).
    pub stale: bool,
}

/// Saving the draft to the server.
#[derive(Clone, Default)]
pub struct SaveWork {
    pub step: Step,
    /// `updated_at_secs` of the last successful save; `None` until the first.
    pub saved_at_secs: Option<i64>,
    /// Edited since the last save was started.
    pub dirty: bool,
    pub in_flight: bool,
    pub revision: i64,
}

#[derive(Clone)]
pub struct ExamState {
    pub saves: Signal<SaveQueue>,
    pub format: FormatId,
    pub exam: Exam,
    /// Index-aligned with `exam.parts`.
    pub work: Vec<PartWork>,
    pub audio: AudioWork,
    pub save: SaveWork,
    /// The saved exams on this server, most recently updated first.
    pub library: Vec<ExamSummary>,
    pub library_error: Option<String>,
    /// What Gemini has billed for this exam so far (from the server's ledger).
    pub spend: Option<ExamUsage>,
    /// Spend on the whole server, and where its API key comes from.
    pub totals: Option<UsageTotals>,
    pub key_source: Option<KeySource>,
    /// Bumped on every new exam run or cancel; a running pipeline compares
    /// against it before every write, so late results of an old run are dropped.
    pub run: u32,
    /// The speaker whose edit dialog is open: (part index, speaker index).
    pub editing_speaker: Option<(usize, usize)>,
    /// New scripts are expressive (`PassageRequest::expressive`): on for a
    /// new exam, as saved for an opened one (off for exams from before 0.8).
    pub expressive: bool,
    /// How this exam's downloads are named (`ui::naming`); not saved, so an
    /// opened exam's first download takes a new time.
    pub naming: DraftNaming,
}

impl AsRef<DraftNaming> for ExamState {
    fn as_ref(&self) -> &DraftNaming {
        &self.naming
    }
}

impl AsMut<DraftNaming> for ExamState {
    fn as_mut(&mut self) -> &mut DraftNaming {
        &mut self.naming
    }
}

impl ExamState {
    pub fn with_queue(saves: Signal<SaveQueue>) -> Self {
        Self::for_format(FormatId::HsgNational, saves)
    }
    fn for_format(format: FormatId, saves: Signal<SaveQueue>) -> Self {
        let exam_format = format.format();
        let title = format!("{} - draft", exam_format.name);
        let exam = Exam::new(exam_format, title, "");
        let work = exam.parts.iter().map(|_| PartWork::default()).collect();
        Self {
            saves,
            format,
            exam,
            work,
            audio: AudioWork::default(),
            save: SaveWork::default(),
            library: Vec::new(),
            library_error: None,
            spend: None,
            totals: None,
            key_source: None,
            run: 0,
            editing_speaker: None,
            expressive: true,
            naming: DraftNaming::default(),
        }
    }

    /// A fresh exam (new id) in the given format that still supersedes runs
    /// in flight and keeps the saved-exams list.
    fn switch_format(&self, format: FormatId) -> Self {
        Self {
            run: self.run + 1,
            library: self.library.clone(),
            totals: self.totals,
            key_source: self.key_source,
            ..Self::for_format(format, self.saves)
        }
    }

    /// Saving starts on its own once there is something worth keeping: a
    /// script, or an explicit Save earlier.
    fn auto_save_armed(&self) -> bool {
        self.save.saved_at_secs.is_some() || self.exam.parts.iter().any(|p| p.passage.is_some())
    }

    /// What the server will keep.
    fn snapshot(&self) -> SavedExam {
        SavedExam {
            revision: self.save.revision,
            exam: self.exam.clone(),
            topics: self.work.iter().map(|w| w.topic.clone()).collect(),
            recording_job: self.audio.job_id.clone(),
            recording: self.audio.track.clone(),
            recording_stale: self.audio.stale,
            expressive: self.expressive,
            created_at_secs: 0,
            updated_at_secs: 0,
        }
    }

    /// The state for a saved exam, with issues recomputed and the recording
    /// restored. `run` is bumped so pipelines of the previous exam stop writing.
    fn open_saved(&self, saved: SavedExam) -> Self {
        let SavedExam {
            revision,
            exam,
            topics,
            recording_job,
            recording,
            recording_stale,
            expressive,
            updated_at_secs,
            ..
        } = saved;
        let work = exam
            .parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let passage_issues = part
                    .passage
                    .as_ref()
                    .map(|p| validate_passage(p, &part.spec, &part.speakers))
                    .unwrap_or_default();
                let task_issues = part
                    .tasks
                    .iter()
                    .flat_map(|t| validate_task(t, part.passage.as_ref()))
                    .collect();
                let tasks_step = if part.tasks.is_empty() {
                    Step::Idle
                } else if part.missing_tasks().is_empty() {
                    Step::Done
                } else {
                    Step::Failed(format!(
                        "Only {} of {} question blocks were generated",
                        part.tasks.len(),
                        part.spec.tasks.len()
                    ))
                };
                PartWork {
                    topic: topics.get(i).cloned().unwrap_or_default(),
                    topic_step: Step::Idle,
                    script_step: if part.passage.is_some() {
                        Step::Done
                    } else {
                        Step::Idle
                    },
                    tasks_step,
                    passage_issues,
                    task_issues,
                }
            })
            .collect();
        let audio = AudioWork {
            step: if recording.is_some() {
                Step::Done
            } else if recording_job.is_some() {
                Step::Running
            } else {
                Step::Idle
            },
            job_id: recording_job,
            progress: if recording.is_some() { 1.0 } else { 0.0 },
            track: recording,
            stale: recording_stale,
        };
        Self {
            saves: self.saves,
            format: exam.format.id,
            exam,
            work,
            audio,
            save: SaveWork {
                step: Step::Done,
                saved_at_secs: (revision > 0).then_some(updated_at_secs),
                revision,
                ..SaveWork::default()
            },
            library: self.library.clone(),
            library_error: None,
            spend: None,
            totals: self.totals,
            key_source: self.key_source,
            run: self.run + 1,
            editing_speaker: None,
            expressive,
            naming: DraftNaming::default(),
        }
    }

    /// Part `i`'s passage issues against its current speakers: after a
    /// speaker edit, or a script kept for new speakers.
    fn revalidate_passage(&mut self, i: usize) {
        let part = &self.exam.parts[i];
        self.work[i].passage_issues = part
            .passage
            .as_ref()
            .map(|p| validate_passage(p, &part.spec, &part.speakers))
            .unwrap_or_default();
    }

    /// The parts whose speakers sound different from the exam recording
    /// (none while there is no recording), by title.
    fn parts_recorded_differently(&self) -> Vec<String> {
        if self.audio.track.is_none() {
            return Vec::new();
        }
        self.exam
            .parts
            .iter()
            .filter(|part| part.recording_stale())
            .map(|part| part.spec.title.clone())
            .collect()
    }

    /// One line under the setup panel about the draft on the server.
    fn save_status(&self) -> (String, bool) {
        match &self.save.step {
            Step::Running => ("Saving...".into(), false),
            Step::Failed(message) => (message.clone(), true),
            _ if self.save.saved_at_secs.is_none() => (
                "Not saved yet. Saving starts on its own once a script exists; press Save to keep the draft now.".into(),
                false,
            ),
            _ if self.save.dirty => ("Unsaved changes".into(), false),
            _ => (
                format!("Saved at {}", local_time(self.save.saved_at_secs.unwrap_or_default())),
                false,
            ),
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
    let catalogue = use_context::<VoiceCatalogueCtx>();
    let exam_id = use_memo(move || state.read().exam.id);

    // Speakers on Auto get voices once the catalogue is here and whenever
    // another exam is opened or started. This is not an edit: nothing is
    // marked stale or saved for it; the next save keeps the voices.
    use_effect(move || {
        let _ = exam_id();
        let Some(voices) = catalogue.voices() else {
            return;
        };
        let line_ups: Vec<Vec<SpeakerConfig>> = state
            .peek()
            .exam
            .parts
            .iter()
            .map(|p| p.speakers.clone())
            .collect();
        if !line_ups.iter().flatten().any(|s| s.voice.is_auto()) {
            return;
        }
        let assigned: Vec<Vec<SpeakerConfig>> = assign_exam_voices(&line_ups, &voices)
            .into_iter()
            .map(|part| part.speakers)
            .collect();
        if assigned != line_ups {
            let mut s = state.write();
            for (part, speakers) in s.exam.parts.iter_mut().zip(assigned) {
                part.speakers = speakers;
            }
        }
    });

    let current = state();
    let busy = current.is_busy();
    let part_count = current.exam.parts.len();
    let exam_issues = validate_exam(&current.exam);
    let has_any_script = current.scripts_done() > 0;
    let all_scripts = current.scripts_done() == part_count;
    let audio_pct = format!("{:.0}", (current.audio.progress * 100.0).clamp(0.0, 100.0));
    let has_recording = current.audio.track.is_some();
    let recorded_differently = current.parts_recorded_differently();
    let (save_text, save_failed) = current.save_status();
    let save_class = if save_failed {
        "save-status failed"
    } else {
        "save-status"
    };

    // The list is fetched after mount so the server render and the browser
    // agree on an empty panel until then.
    use_future(move || refresh_library(state));
    use_future(move || async move {
        refresh_spend(state).await;
        if let Ok(status) = api_key_status().await {
            state.write().key_source = Some(status.source);
        }
    });

    let handle_save = move |_| request_save(state);

    // A fresh exam with a new id; the current one is saved first if it has
    // unsaved edits and saving was already on.
    let handle_new_exam = move |_| {
        flush_before_leaving(state);
        let fresh = state.peek().switch_format(state.peek().format);
        state.set(fresh);
    };

    let handle_open = move |id: Uuid| open_exam(state, id);
    let handle_delete = move |id: Uuid| delete_saved_exam(state, id);

    // The one-click pipeline: topics for parts that have none, then every
    // script at once, then every part's questions at once beside the single
    // exam recording job.
    let handle_generate_exam = move |_| {
        invalidate_open(state);
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
                    exam_audio_request(&s, None)
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
        invalidate_open(state);
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

    let handle_render_audio = move |_| render_exam_audio(state, None);

    let handle_check_audio = move |_| {
        let Some(job_id) = state.peek().audio.job_id.clone() else {
            return;
        };
        let run = state.peek().run;
        spawn_forever(fetch_exam_audio(state, run, job_id, true));
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
                                        flush_before_leaving(state);
                                        let fresh = state.peek().switch_format(format);
                                        state.set(fresh);
                                    }
                                },
                                for id in FormatId::ALL {
                                    option { value: "{id.key()}", selected: id == current.format, "{id.format().name}" }
                                }
                            }
                        }
                    }
                    div { class: "config-group",
                        label { class: "input-label", "Title:" }
                        input {
                            class: "form-input",
                            value: "{current.exam.title}",
                            oninput: move |evt| {
                                state.write().exam.title = evt.value();
                                note_edit(state);
                            },
                        }
                    }
                }
                label { class: "input-label", "Theme for topic suggestions (optional):" }
                textarea {
                    class: "input-textarea small",
                    placeholder: "Example: news listening about cities and the environment",
                    value: "{current.exam.theme}",
                    oninput: move |evt| {
                        state.write().exam.theme = evt.value();
                        note_edit(state);
                    },
                }
                label { class: "expressive-toggle",
                    input {
                        r#type: "checkbox",
                        checked: current.expressive,
                        disabled: busy,
                        onchange: move |evt| {
                            state.write().expressive = evt.checked();
                            note_edit(state);
                        },
                    }
                    " Expressive delivery (sighs, laughs)"
                }
                p { class: "muted", "New scripts may carry a few sighs, coughs, laughs and chuckles that the voices perform. Transcripts, questions and downloads show only the words." }
                AutoDownloadToggle {}
                p { class: "{save_class}", "{save_text}" }
                {spend_view(current.spend)}
                div { class: "download-buttons",
                    button {
                        class: "download-button info",
                        disabled: busy,
                        onclick: handle_suggest_all,
                        "Suggest topics for empty parts"
                    }
                    button {
                        class: "download-button secondary",
                        disabled: busy || !all_scripts || !catalogue.is_ready(),
                        onclick: handle_render_audio,
                        "Render exam audio"
                    }
                    button {
                        class: "download-button primary",
                        disabled: current.save.in_flight,
                        onclick: handle_save,
                        "Save"
                    }
                    button {
                        class: "download-button secondary",
                        disabled: busy,
                        onclick: handle_new_exam,
                        "New exam"
                    }
                }
            }

            div { class: "generator-panel",
                h2 { class: "panel-header", "Saved exams" }
                p { class: "panel-help",
                    "Kept on this server. Open one to continue where you left off; deleting an exam also deletes its recording."
                }
                if let Some(totals) = current.totals {
                    p { class: "panel-help",
                        "Gemini spend on this server: {totals.last_24h.cost_text()} in the last 24 hours, {totals.last_30_days.cost_text()} in the last 30 days."
                        if let Some(source) = current.key_source {
                            " API key from {source.describe()}."
                        }
                    }
                }
                if let Some(error) = current.library_error.clone() {
                    div { class: "error-message", "{error}" }
                }
                ExamLibrary {
                    exams: current.library.clone(),
                    current: current.exam.id,
                    busy,
                    onopen: handle_open,
                    ondelete: handle_delete,
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
                        let speakers = part.speakers.clone();
                        let voices = voices_summary(&speakers);
                        let warnings = speaker_warnings(&spec, &speakers);
                        let drifted = passage
                            .as_ref()
                            .is_some_and(|p| p.speakers_changed(&speakers).script);
                        let recording_stale = has_recording && part.recording_stale();
                        let number = spec.number;
                        let exam_uuid = current.exam.id;
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
                                    oninput: move |evt| {
                                        state.write().work[i].topic = evt.value();
                                        note_edit(state);
                                    },
                                }
                                if let Step::Failed(message) = work.topic_step.clone() {
                                    div { class: "error-message", "{message}" }
                                }
                                details { class: "preview part-voices",
                                    summary { "Voices: {voices}" }
                                    SpeakerCards {
                                        speakers: speakers.clone(),
                                        disabled: busy,
                                        warnings,
                                        exam: Some(exam_uuid),
                                        onedit: move |k| state.write().editing_speaker = Some((i, k)),
                                        onvoice: move |(k, choice): (usize, VoiceChoice)| {
                                            let mut list = state.peek().exam.parts[i].speakers.clone();
                                            if let Some(speaker) = list.get_mut(k) {
                                                *speaker = with_choice(speaker.clone(), choice);
                                            }
                                            set_part_speakers(state, catalogue, i, list);
                                        },
                                        onspend: move |_| {
                                            spawn_forever(refresh_spend(state));
                                        },
                                    }
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
                                        onclick: move |_| rewrite_part(state, i),
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
                                    if has_recording {
                                        button {
                                            class: "download-button info",
                                            disabled: busy || !all_scripts || !catalogue.is_ready(),
                                            title: "Renders the exam recording again, reading this part afresh instead of reusing its earlier take. Pays for this part's speech again; the other parts and the announcements are reused.",
                                            onclick: move |_| render_exam_audio(state, Some(number)),
                                            "New take of this part"
                                        }
                                    }
                                }
                                if drifted {
                                    div { class: "note-box",
                                        span { class: "note-icon", "!" }
                                        div { class: "note-body",
                                            span { "The speakers changed after this script was written; names, pronouns and wording may no longer fit." }
                                            div { class: "note-actions",
                                                button {
                                                    class: "voice-button",
                                                    disabled: busy,
                                                    onclick: move |_| rewrite_part(state, i),
                                                    "Rewrite script and questions"
                                                }
                                                button {
                                                    class: "voice-button",
                                                    disabled: busy,
                                                    onclick: move |_| keep_part_script(state, i),
                                                    "Keep this script"
                                                }
                                            }
                                        }
                                    }
                                }
                                if recording_stale {
                                    div { class: "note-box",
                                        span { class: "note-icon", "!" }
                                        span { "A voice changed after this recording was made; render it again." }
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
                    if !recorded_differently.is_empty() {
                        div { class: "note-box",
                            span { class: "note-icon", "!" }
                            span {
                                "A voice changed in {recorded_differently.join(\", \")} after this recording was made; render it again."
                            }
                        }
                    }
                    if let Some(error) = catalogue.error() {
                        div { class: "error-message", "Could not load the voices: {error}" }
                        button {
                            class: "voice-button",
                            onclick: move |_| catalogue.retry(),
                            "Try again"
                        }
                    }
                    {audio_body}
                    if let Some(track) = current.audio.track.clone() {
                        AudioPlayerSection {
                            src: audio_url(&track.location),
                            duration_ms: Some(track.duration_ms),
                            ondownload: move |_| download_exam_wav(state),
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
                        "One document, as Word (DOCX, laid out like the paper with answer boxes) or Markdown: \
                         every part's questions, the answer key and the transcripts."
                    }
                    IssueList { issues: exam_issues.clone() }
                    div { class: "download-buttons",
                        button {
                            class: "download-button primary",
                            disabled: !has_any_script,
                            onclick: move |_| download_exam_docx(state),
                            if exam_issues.is_empty() { "Download exam (DOCX)" } else { "Download draft (DOCX, incomplete)" }
                        }
                        button {
                            class: "download-button info",
                            disabled: !has_any_script,
                            onclick: move |_| {
                                let document = markdown::render_exam(&state.peek().exam);
                                download_exam(state, Content::Text(document), "md");
                            },
                            "Download exam (Markdown)"
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

            if let Some((i, k)) = current.editing_speaker {
                if let Some(speaker) = current.exam.parts.get(i).and_then(|p| p.speakers.get(k)).cloned() {
                    SpeakerEditModal {
                        speaker,
                        taken: current
                            .exam
                            .parts
                            .get(i)
                            .map(|p| voices_of_others(&p.speakers, k))
                            .unwrap_or_default(),
                        exam: Some(current.exam.id),
                        onclose: move |_| state.write().editing_speaker = None,
                        onsave: move |updated: SpeakerConfig| {
                            let Some(mut list) = state.peek().exam.parts.get(i).map(|p| p.speakers.clone()) else {
                                return;
                            };
                            if k < list.len() {
                                list[k] = updated;
                            }
                            set_part_speakers(state, catalogue, i, list);
                        },
                        onspend: move |_| {
                            spawn_forever(refresh_spend(state));
                        },
                    }
                }
            }

            if let Some((message, submessage)) = current.busy_message() {
                LoadingPopup {
                    message,
                    submessage,
                    oncancel: move |_| { state.write().cancel(); note_edit(state); },
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

/// What the exam's downloads are named after: the format, and the theme and
/// the topic of every part that has a script (what the files hold; typing a
/// topic for another part changes nothing).
fn exam_name(state: &ExamState) -> NameRequest {
    let exam = &state.exam;
    let topics = exam
        .parts
        .iter()
        .filter_map(|part| part.passage.as_ref().map(|p| p.topic.as_str()));
    let source = std::iter::once(exam.theme.as_str())
        .chain(topics)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    NameRequest {
        test_type: test_type(&exam.format, None),
        source,
        exam: Some(exam.id),
    }
}

/// Downloads one file of the exam: its content as it is now, its name once
/// the summary is here (`ui::naming`). Runs on the root scope, like the
/// pipelines, so it finishes on the other page too.
fn download_exam(state: Signal<ExamState>, content: Content, extension: &'static str) {
    let request = exam_name(&state.peek());
    spawn_forever(async move {
        download(state, request, content, None, extension).await;
        // A summary asked for meanwhile is in the ledger by now.
        refresh_spend(state).await;
    });
}

/// Every part's paper, the key and the transcripts as DOCX.
fn download_exam_docx(state: Signal<ExamState>) {
    let bytes = docx::render_exam_docx(&state.peek().exam);
    download_exam(state, Content::Bytes(bytes, docx::DOCX_MIME), "docx");
}

/// The exam recording, straight from the server.
fn download_exam_wav(state: Signal<ExamState>) {
    let link = state
        .peek()
        .audio
        .track
        .as_ref()
        .map(|track| Content::Link(audio_url(&track.location)));
    if let Some(link) = link {
        download_exam(state, link, "wav");
    }
}

/// True while `run` is still the run whose results the state expects.
fn still_current(state: Signal<ExamState>, run: u32) -> bool {
    state.peek().run == run
}

/// Refreshes the saved-exams list from the server.
async fn refresh_library(mut state: Signal<ExamState>) {
    let mut queue = state.peek().saves;
    let epoch = {
        let mut q = queue.write();
        q.library_epoch = q.library_epoch.wrapping_add(1);
        q.library_epoch
    };
    let result = list_exams().await;
    if queue.peek().library_epoch != epoch {
        return;
    }
    match result {
        Ok(library) => {
            let mut s = state.write();
            let mut latest = library;
            for summary in &s.library {
                if let Some(row) = latest.iter_mut().find(|r| r.id == summary.id)
                    && row.revision < summary.revision
                {
                    *row = summary.clone();
                }
            }
            latest.retain(|e| !queue.peek().deleted.contains(&e.id));
            s.library = latest;
            s.library_error = None;
        }
        Err(e) => {
            state.write().library_error = Some(format!("Could not list the saved exams: {e}"))
        }
    }
}

/// Fetches what the open exam has cost and the server's totals. The exam's
/// figure is dropped if another exam was opened meanwhile.
async fn refresh_spend(mut state: Signal<ExamState>) {
    let id = state.peek().exam.id;
    let (spend, totals) = join(exam_usage(id), usage_totals()).await;
    let mut s = state.write();
    if let Ok(spend) = spend
        && s.exam.id == id
    {
        s.spend = Some(spend);
    }
    if let Ok(totals) = totals {
        s.totals = Some(totals);
    }
}

/// The exam's Gemini spend per step beside its budget, token counts on hover.
/// Going over the budget only warns; nothing is blocked.
fn spend_view(spend: Option<ExamUsage>) -> Element {
    let Some(spend) = spend.filter(|s| !s.total().is_empty()) else {
        return rsx! {
            p { class: "spend-status", "Gemini spend for this exam: nothing yet." }
        };
    };
    let total = spend.total();
    let budget = spend
        .budget_text()
        .map(|b| format!(" of {b}"))
        .unwrap_or_default();
    let detail = format!(
        "{} requests, {} reused from earlier recordings; tokens: {} in ({} cached), {} out, {} thinking",
        total.requests,
        total.reused,
        total.input_tokens,
        total.cached_tokens,
        total.output_tokens,
        total.thinking_tokens
    );
    // Voices and file names appear only once something was spent on them.
    let steps = UsageStep::ALL
        .into_iter()
        .filter(|step| step.always_listed() || !spend.step(*step).is_empty())
        .map(|step| format!("{} {}", step.label(), spend.step(step).cost_text()))
        .collect::<Vec<_>>()
        .join(", ");
    rsx! {
        p { class: "spend-status", title: "{detail}",
            "Gemini spend for this exam: "
            strong { "{total.cost_text()}" }
            "{budget} ({steps})"
        }
        if spend.over_budget() {
            div { class: "note-box",
                span { class: "note-icon", "!" }
                span {
                    "This exam has cost more than its budget{budget} (EXAM_BUDGET_USD). Nothing is blocked, but every regeneration adds to it."
                }
            }
        }
    }
}

/// Keeps the list sorted by last update, newest first.
fn upsert_summary(library: &mut Vec<ExamSummary>, summary: ExamSummary) {
    if library
        .iter()
        .any(|e| e.id == summary.id && e.revision > summary.revision)
    {
        return;
    }
    library.retain(|e| e.id != summary.id);
    library.push(summary);
    library.sort_by(|a, b| {
        b.updated_at_secs
            .cmp(&a.updated_at_secs)
            .then_with(|| a.title.cmp(&b.title))
    });
}

/// Copies edits into the layout-owned queue before any await or navigation.
fn invalidate_open(state: Signal<ExamState>) {
    let mut queue = state.peek().saves;
    queue.write().invalidate_open();
}
fn stage_current(state: Signal<ExamState>, force: bool) -> Uuid {
    let (mut queue, snapshot, armed) = {
        let s = state.peek();
        (s.saves, s.snapshot(), force || s.auto_save_armed())
    };
    let id = snapshot.exam.id;
    queue.write().stage(snapshot, armed);
    sync_save_status(state);
    id
}
fn sync_save_status(mut state: Signal<ExamState>) {
    let (queue, id) = {
        let s = state.peek();
        (s.saves, s.exam.id)
    };
    let draft = queue.peek().drafts.get(&id).cloned();
    if let Some(draft) = draft {
        let mut s = state.write();
        s.save.dirty = draft.dirty();
        s.save.in_flight = draft.status == SaveStatus::Sending;
        s.save.revision = draft.snapshot.revision;
        s.save.saved_at_secs =
            (draft.snapshot.revision > 0).then_some(draft.snapshot.updated_at_secs);
        s.save.step = match draft.status {
            SaveStatus::Sending => Step::Running,
            SaveStatus::Failed(message) | SaveStatus::Invalid(message) => Step::Failed(message),
            SaveStatus::Conflict => Step::Failed(
                "This exam changed in another tab. Resolve it in the save panel.".into(),
            ),
            SaveStatus::Deleted => {
                Step::Failed("This exam was deleted. Save your draft as a new exam.".into())
            }
            SaveStatus::Ready if draft.dirty() => Step::Idle,
            SaveStatus::Ready => Step::Done,
        };
        // Only normalize a reference if it has not since been replaced locally.
        if s.audio.job_id.is_some()
            && draft.snapshot.recording_job.is_none()
            && draft.warning.is_some()
        {
            s.audio.job_id = None;
            s.audio.track = None;
            s.audio.step = Step::Failed(draft.warning.unwrap_or_default());
        }
    }
}
fn request_save(state: Signal<ExamState>) {
    let id = stage_current(state, true);
    start_save(state, id, true);
}
fn start_save(state: Signal<ExamState>, id: Uuid, retry: bool) {
    let mut queue = state.peek().saves;
    let request = queue.write().begin(id, retry);
    sync_save_status(state);
    if let Some(request) = request {
        spawn_forever(persist(state, id, request));
    }
}
async fn persist(
    mut state: Signal<ExamState>,
    id: Uuid,
    mut request: crate::application::exams::SaveRequest,
) {
    let mut queue = state.peek().saves;
    loop {
        let result = save_exam(request)
            .await
            .map_err(|e| format!("Save failed: {e}"));
        if let Ok(SaveOutcome::Saved { summary, .. }) = &result {
            upsert_summary(&mut state.write().library, summary.clone());
        }
        if matches!(result, Ok(SaveOutcome::Deleted)) {
            state.write().library.retain(|summary| summary.id != id);
        }
        queue.write().finish(id, result);
        sync_save_status(state);
        let next = queue.write().begin(id, false);
        let Some(next) = next else {
            break;
        };
        request = next;
        sync_save_status(state);
    }
    queue.write().evict_clean(state.peek().exam.id);
}
fn auto_save(state: Signal<ExamState>) {
    spawn_forever(refresh_spend(state));
    let id = stage_current(state, false);
    start_save(state, id, false);
}
fn note_edit(state: Signal<ExamState>) {
    let mut queue = state.peek().saves;
    queue.write().invalidate_open();
    let id = stage_current(state, false);
    let sequence = queue.peek().drafts[&id].sequence;
    spawn_forever(async move {
        sleep_ms(EDIT_DEBOUNCE_MS).await;
        if queue
            .peek()
            .drafts
            .get(&id)
            .is_some_and(|d| d.sequence == sequence)
        {
            start_save(state, id, false);
        }
    });
}
fn flush_before_leaving(state: Signal<ExamState>) {
    let mut queue = state.peek().saves;
    queue.write().invalidate_open();
    if state.peek().save.dirty {
        let id = stage_current(state, false);
        start_save(state, id, false);
    }
}
fn install_exam(mut state: Signal<ExamState>, saved: SavedExam) {
    let fresh = state.peek().open_saved(saved);
    let resume = fresh
        .audio
        .track
        .is_none()
        .then(|| fresh.audio.job_id.clone())
        .flatten()
        .map(|job| (job, fresh.run));
    state.set(fresh);
    sync_save_status(state);
    let mut queue = state.peek().saves;
    queue.write().evict_clean(state.peek().exam.id);
    spawn_forever(refresh_spend(state));
    if let Some((job, run)) = resume {
        spawn_forever(fetch_exam_audio(state, run, job, false));
    }
}
fn open_exam(mut state: Signal<ExamState>, id: Uuid) {
    flush_before_leaving(state);
    let mut queue = state.peek().saves;
    let token = queue.write().invalidate_open();
    let local = queue.peek().drafts.get(&id).map(|d| d.snapshot.clone());
    if let Some(saved) = local {
        install_exam(state, saved);
        return;
    }
    spawn_forever(async move {
        let result = load_exam(id.to_string()).await;
        if !queue.peek().accepts_open(token) {
            return;
        }
        match result {
            Ok(saved) => install_exam(state, saved),
            Err(e) => state.write().library_error = Some(format!("Could not open the exam: {e}")),
        }
    });
}
fn delete_saved_exam(mut state: Signal<ExamState>, id: Uuid) {
    let mut queue = state.peek().saves;
    if queue.peek().drafts.get(&id).is_some_and(|d| d.deleting) {
        return;
    }
    queue.write().invalidate_open();
    if state.peek().exam.id == id && state.peek().save.dirty {
        stage_current(state, false);
    }
    let revision = queue
        .peek()
        .drafts
        .get(&id)
        .map(|d| d.snapshot.revision)
        .or_else(|| {
            state
                .peek()
                .library
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.revision)
        });
    let Some(mut revision) = revision else {
        return;
    };
    let confirmed_sequence = queue.peek().drafts.get(&id).map(|d| d.sequence);
    if let Some(draft) = queue.write().drafts.get_mut(&id) {
        draft.deleting = true;
    }
    spawn_forever(async move {
        loop {
            let draft = queue.peek().drafts.get(&id).cloned();
            match draft {
                Some(d) if d.status == SaveStatus::Sending => sleep_ms(25).await,
                Some(d) => {
                    if d.pending.is_some() {
                        queue.write().drafts.get_mut(&id).unwrap().deleting = false;
                        state.write().library_error = Some(
                            "Resolve the failed save with Retry before deleting this exam.".into(),
                        );
                        return;
                    }
                    revision = d.snapshot.revision;
                    break;
                }
                None => break,
            }
        }
        match delete_exam(id.to_string(), revision).await {
            Ok(DeleteOutcome::Deleted | DeleteOutcome::AlreadyDeleted) => {
                let edited_after_confirmation = queue
                    .peek()
                    .drafts
                    .get(&id)
                    .is_some_and(|d| Some(d.sequence) != confirmed_sequence);
                if edited_after_confirmation {
                    let mut q = queue.write();
                    q.deleted.insert(id);
                    q.library_epoch = q.library_epoch.wrapping_add(1);
                    if let Some(d) = q.drafts.get_mut(&id) {
                        d.status = SaveStatus::Deleted;
                        d.deleting = false;
                        d.pending = None;
                    }
                } else {
                    queue.write().forget_deleted(id);
                }
                let mut s = state.write();
                s.library.retain(|e| e.id != id);
                s.library_error = None;
                if s.exam.id == id && !edited_after_confirmation {
                    *s = s.switch_format(s.format);
                }
            }
            result => {
                if matches!(result, Ok(DeleteOutcome::Conflict { .. })) {
                    if state.peek().exam.id == id {
                        stage_current(state, false);
                    }
                    if let Some(d) = queue.write().drafts.get_mut(&id) {
                        d.status = SaveStatus::Conflict;
                        if !d.dirty() {
                            d.sequence += 1;
                        }
                    }
                }
                if let Some(d) = queue.write().drafts.get_mut(&id) {
                    d.deleting = false;
                }
                state.write().library_error = Some(match result {
                    Ok(DeleteOutcome::Conflict { .. }) => "This exam changed. Open the latest version before confirming deletion again.".into(),
                    Err(e) => format!("Could not delete the exam: {e}"),
                    _ => unreachable!(),
                });
            }
        }
        sync_save_status(state);
    });
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
        expressive: state.expressive,
    };
    request.validate().map_err(|e| e.to_string())?;
    Ok(request)
}

/// Every part's passage as one `ExamAudioRequest`; the domain names the
/// first part that has no script yet. Part `fresh_part`, if any, is read
/// afresh ("New take of this part").
fn exam_audio_request(
    state: &ExamState,
    fresh_part: Option<u8>,
) -> Result<ExamAudioRequest, String> {
    let parts = state
        .exam
        .parts
        .iter()
        .filter_map(|p| {
            p.passage.clone().map(|passage| AudioRequest {
                passage,
                speakers: p.speakers.clone(),
                fresh: fresh_part == Some(p.spec.number),
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

/// Renders the exam recording from the scripts as they are; with
/// `fresh_part`, that part gets a new take instead of its earlier one.
fn render_exam_audio(mut state: Signal<ExamState>, fresh_part: Option<u8>) {
    invalidate_open(state);
    let request = {
        let s = state.peek();
        exam_audio_request(&s, fresh_part)
    };
    match request {
        Ok(request) => {
            let run = state.peek().run;
            spawn_forever(run_exam_audio(state, run, request));
        }
        Err(message) => state.write().audio.step = Step::Failed(message),
    }
}

/// Writes part `i`'s script again, then its questions.
fn rewrite_part(state: Signal<ExamState>, i: usize) {
    invalidate_open(state);
    let run = state.peek().run;
    spawn_forever(async move {
        if run_part_script(state, run, i).await {
            run_part_tasks(state, run, i).await;
        }
    });
}

/// "Keep this script": the teacher accepts part `i`'s script for its
/// current speakers, so it is noted as written for them.
fn keep_part_script(mut state: Signal<ExamState>, i: usize) {
    {
        let mut s = state.write();
        let Some(part) = s.exam.parts.get_mut(i) else {
            return;
        };
        let Some(passage) = part.passage.take() else {
            return;
        };
        part.passage = Some(passage.for_speakers(&part.speakers));
        s.revalidate_passage(i);
    }
    note_edit(state);
}

/// The one place a teacher's speaker edit lands on the exam page (the edit
/// dialog, "Another voice", "Automatic"). The part's speakers get voices
/// again, preferring voices no other part uses; the passage issues follow
/// the new line-up; the edit is saved. Nothing is flagged: a script or
/// recording made for other speakers is told apart by `written_for` and
/// `recorded_for`.
fn set_part_speakers(
    mut state: Signal<ExamState>,
    catalogue: VoiceCatalogueCtx,
    i: usize,
    speakers: Vec<SpeakerConfig>,
) {
    let voices = catalogue.voices();
    let changed = {
        let mut s = state.write();
        s.editing_speaker = None;
        if i >= s.exam.parts.len() {
            return;
        }
        let speakers = match voices {
            Some(voices) => {
                let elsewhere: Vec<String> = s
                    .exam
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .flat_map(|(_, p)| p.speakers.iter().filter_map(SpeakerConfig::voice_id))
                    .map(str::to_string)
                    .collect();
                assign_voices(&speakers, &voices, &elsewhere).speakers
            }
            None => speakers,
        };
        if s.exam.parts[i].speakers == speakers {
            false
        } else {
            s.exam.parts[i].speakers = speakers;
            s.revalidate_passage(i);
            true
        }
    };
    if changed {
        note_edit(state);
    }
}

async fn suggest_part_topic(mut state: Signal<ExamState>, run: u32, i: usize) {
    invalidate_open(state);
    let (format, number, theme, exam_id) = {
        let s = state.peek();
        (
            s.format,
            s.exam.parts[i].spec.number,
            s.exam.theme.clone(),
            s.exam.id,
        )
    };
    state.write().work[i].topic_step = Step::Running;
    let outcome = suggest_topic(format, number, theme, Some(exam_id)).await;
    if !still_current(state, run) {
        return;
    }
    let suggested = {
        let mut s = state.write();
        match outcome {
            Ok(topic) => {
                s.work[i].topic = topic;
                s.work[i].topic_step = Step::Done;
                true
            }
            Err(e) => {
                s.work[i].topic_step = Step::Failed(format!("Could not suggest a topic: {e}"));
                false
            }
        }
    };
    if suggested {
        auto_save(state);
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
        s.naming.new_draft();
    }
    stage_current(state, false);
    let exam_id = state.peek().exam.id;
    let outcome = generate_passage(request, Some(exam_id)).await;
    if !still_current(state, run) {
        return false;
    }
    let usable = {
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
    };
    auto_save(state);
    // The summary for the download names, once the last script of a run is
    // in: one request for the exam, not one per part.
    let (all_written, any_script) = {
        let s = state.peek();
        (
            s.work.iter().all(|w| !w.script_step.is_running()),
            s.exam.parts.iter().any(|p| p.passage.is_some()),
        )
    };
    if all_written && any_script {
        let request = exam_name(&state.peek());
        prefetch(state, request.source, request.exam);
    }
    usable
}

/// Generates every task block of part `i`, one after another.
async fn run_part_tasks(mut state: Signal<ExamState>, run: u32, i: usize) {
    let (format, number, passage, speakers, task_count, exam_id) = {
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
            s.exam.id,
        )
    };
    {
        let mut s = state.write();
        // New questions for a part get a new download name; a part's first
        // ones keep the name the exam's files may already have.
        if !s.exam.parts[i].tasks.is_empty() {
            s.naming.new_draft();
        }
        s.exam.parts[i].tasks.clear();
        s.work[i].task_issues.clear();
        s.work[i].tasks_step = Step::Running;
    }
    stage_current(state, false);
    for task_index in 0..task_count {
        let request = TaskRequest {
            format,
            part: number,
            task_index,
            passage: passage.clone(),
            speakers: speakers.clone(),
        };
        let outcome = generate_task(request, Some(exam_id)).await;
        if !still_current(state, run) {
            return;
        }
        match outcome {
            Ok(draft) => {
                let mut s = state.write();
                s.exam.parts[i].tasks.push(draft.task);
                s.work[i].task_issues.extend(draft.issues);
                drop(s);
                stage_current(state, false);
            }
            Err(e) => {
                state.write().work[i].tasks_step =
                    Step::Failed(format!("Question generation failed: {e}"));
                auto_save(state);
                return;
            }
        }
    }
    state.write().work[i].tasks_step = Step::Done;
    auto_save(state);
    // Only the last part to finish sees the exam complete.
    if prefs::auto_download() && state.peek().exam.is_complete() {
        download_exam_docx(state);
    }
}

/// Starts the exam recording job and waits for it.
async fn run_exam_audio(mut state: Signal<ExamState>, run: u32, request: ExamAudioRequest) {
    {
        let mut s = state.write();
        // Recording again, or a new take of a part, gets a new download name.
        // A first recording, or one replacing a recording a new script made
        // stale (that script started a new name already), keeps the name.
        let current = s.audio.track.is_some() && !s.audio.stale;
        if current || request.parts.iter().any(|p| p.fresh) {
            s.naming.new_draft();
        }
        s.audio = AudioWork {
            step: Step::Running,
            ..AudioWork::default()
        };
    }
    stage_current(state, false);
    let exam_id = state.peek().exam.id;
    let numbers: Vec<u8> = request.parts.iter().map(|p| p.passage.part).collect();
    let started = start_exam_audio(request, Some(exam_id)).await;
    if !still_current(state, run) {
        return;
    }
    let started = match started {
        Ok(started) => started,
        Err(e) => {
            state.write().audio.step = Step::Failed(format!("Could not start the recording: {e}"));
            return;
        }
    };
    {
        // Keep the voices the server records with (the same rule as here,
        // so normally nothing changes) and note them as what each part is
        // recorded with; the save below stores both.
        let mut s = state.write();
        for (number, speakers) in numbers.into_iter().zip(started.parts) {
            if let Some(part) = s.exam.parts.iter_mut().find(|p| p.spec.number == number) {
                if part.speakers != speakers {
                    part.speakers = speakers.clone();
                }
                part.recorded_for = speakers;
            }
        }
        s.audio.job_id = Some(started.job_id.clone());
    }
    auto_save(state);
    fetch_exam_audio(state, run, started.job_id, true).await;
}

/// Waits for a started exam job, reporting progress; also behind "Check again"
/// and an opened exam whose recording was still running. With `auto` the
/// finished WAV is downloaded if the teacher asked for that.
async fn fetch_exam_audio(mut state: Signal<ExamState>, run: u32, job_id: String, auto: bool) {
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
    let ready = {
        let mut s = state.write();
        match outcome {
            Ok(job) => match job.track {
                Some(track) => {
                    s.audio.track = Some(track);
                    s.audio.progress = 1.0;
                    s.audio.stale = false;
                    s.audio.step = Step::Done;
                    true
                }
                None => {
                    s.audio.step =
                        Step::Failed("The recording finished but its file is missing".into());
                    false
                }
            },
            Err(message) => {
                s.audio.step = Step::Failed(message);
                false
            }
        }
    };
    if ready {
        auto_save(state);
        if auto && prefs::auto_download() {
            download_exam_wav(state);
        }
    } else {
        // A failed recording was still billed for what it read.
        spawn_forever(refresh_spend(state));
    }
}

/// Session-wide controls remain mounted on both routes.
#[component]
pub fn SaveStatusPanel() -> Element {
    let state = use_context::<Signal<ExamState>>();
    let mut queue = use_context::<Signal<SaveQueue>>();
    let navigator = use_navigator();
    let mut confirmation = use_signal(|| None::<(Uuid, SavedExam, bool)>);
    let mut message = use_signal(|| None::<String>);
    let mut resolving = use_signal(|| false);
    use_before_unload(queue);
    let active = use_memo(move || state.read().exam.id);
    use_effect(move || queue.write().evict_clean(active()));
    let busy = state.read().is_busy();
    let mut drafts: Vec<_> = queue
        .read()
        .drafts
        .iter()
        .filter(|(_, d)| d.dirty() || d.warning.is_some())
        .map(|(id, d)| (*id, d.clone()))
        .collect();
    drafts.sort_by(|a, b| {
        a.1.snapshot
            .exam
            .title
            .cmp(&b.1.snapshot.exam.title)
            .then(a.0.cmp(&b.0))
    });
    let count = drafts.len();
    let mut resolve = move |id: Uuid, overwrite: bool| {
        resolving.set(true);
        message.set(None);
        spawn_forever(async move {
            match load_exam(id.to_string()).await {
                Ok(saved) => confirmation.set(Some((id, saved, overwrite))),
                Err(e) => message.set(Some(format!("Could not load the latest exam: {e}"))),
            }
            resolving.set(false);
        });
    };
    rsx! {
        if count > 0 || message().is_some() || state.read().library_error.is_some() {
            section { class: "session-saves", "aria-label": "Session drafts", "aria-live": "polite",
                strong { "Session drafts: {count} need attention" }
                p { "Drafts stay in this tab until saved. Keep this tab open while saving or resolving an error." }
                if let Some(error) = message() { p { role: "alert", "{error}" } }
                if let Some(error) = state.read().library_error.clone() { p { role: "alert", "{error}" } }
                for (id, draft) in drafts {
                    div { key: "{id}", class: "session-draft",
                        strong { "{draft.snapshot.exam.title}" }
                        span { "{draft_status(&draft)}" }
                        button { disabled: busy || draft.deleting, onclick: move |_| {
                            open_exam(state, id); navigator.push(crate::Route::ExamView {});
                        }, "Open draft" }
                        if matches!(draft.status, SaveStatus::Ready | SaveStatus::Failed(_)) {
                            button { disabled: draft.deleting, onclick: move |_| {
                                if let Some(d) = queue.write().drafts.get_mut(&id) { d.armed = true; }
                                start_save(state, id, true);
                            }, "Save / Retry" }
                        }
                        if matches!(draft.status, SaveStatus::Conflict) {
                            button { disabled: resolving() || busy, onclick: move |_| resolve(id, true), "Overwrite server version" }
                            button { disabled: resolving() || busy, onclick: move |_| resolve(id, false), "Open latest version" }
                            span { "Or leave this draft here and resolve it later." }
                        }
                        if matches!(draft.status, SaveStatus::Deleted) {
                            button { disabled: busy, onclick: move |_| {
                                flush_before_leaving(state);
                                let original = queue.peek().drafts.get(&id).map(|d| d.snapshot.clone());
                                if let Some(mut saved) = original {
                                    saved.exam.id = Uuid::new_v4(); saved.revision = 0;
                                    saved.created_at_secs = 0; saved.updated_at_secs = 0;
                                    let new_id = saved.exam.id;
                                    queue.write().stage(saved.clone(), true);
                                    queue.write().forget_deleted(id);
                                    install_exam(state, saved);
                                    start_save(state, new_id, false);
                                    navigator.push(crate::Route::ExamView {});
                                }
                            }, "Save as new exam" }
                        }
                        if let Some(warning) = draft.warning {
                            p { role: "alert", "{warning}" }
                            button { onclick: move |_| {
                                if let Some(d) = queue.write().drafts.get_mut(&id) { d.warning = None; }
                            }, "Dismiss notice" }
                        }
                    }
                }
            }
        }
        if let Some((id, saved, overwrite)) = confirmation() {
            div { class: "save-confirm-backdrop",
                section { class: "save-confirm", role: "dialog", "aria-modal": "true", "aria-label": "Confirm draft resolution",
                    h2 { if overwrite { "Replace the server version?" } else { "Discard your local changes?" } }
                    p { "{saved.exam.title} — version {saved.revision}, updated at {local_time(saved.updated_at_secs)}" }
                    p { if overwrite { "Your draft will replace this version. If it changes again, saving will stop with another conflict." }
                        else { "Your unsaved edits in this tab will be discarded and the version shown above will open." } }
                    button { onclick: move |_| confirmation.set(None), "Leave for later" }
                    button { disabled: busy, onclick: move |_| {
                        confirmation.set(None);
                        if overwrite {
                            queue.write().overwrite(id, saved.revision);
                            start_save(state, id, false);
                        } else {
                            flush_before_leaving(state);
                            queue.write().drafts.remove(&id);
                            install_exam(state, saved.clone());
                            navigator.push(crate::Route::ExamView {});
                        }
                    }, if overwrite { "Confirm overwrite" } else { "Discard edits and open" } }
                }
            }
        }
    }
}
fn draft_status(draft: &crate::ui::save_queue::Draft) -> String {
    if draft.deleting {
        return "Waiting to delete…".into();
    }
    match &draft.status {
        SaveStatus::Ready if draft.dirty() => "Unsaved changes".into(),
        SaveStatus::Ready => "Saved".into(),
        SaveStatus::Sending => "Saving…".into(),
        SaveStatus::Failed(error) | SaveStatus::Invalid(error) => error.clone(),
        SaveStatus::Conflict => "Changed in another tab. Your draft is kept here.".into(),
        SaveStatus::Deleted => "Deleted on the server. Your draft is kept here.".into(),
    }
}
fn use_before_unload(_queue: Signal<SaveQueue>) {
    #[cfg(target_arch = "wasm32")]
    {
        use std::{cell::Cell, rc::Rc};
        let flag = use_hook(|| Rc::new(Cell::new(false)));
        let monitor = flag.clone();
        use_effect(move || monitor.set(_queue.read().needs_warning()));
        let _guard = use_hook(move || Rc::new(UnloadGuard::new(flag)));
    }
}
#[cfg(target_arch = "wasm32")]
struct UnloadGuard(wasm_bindgen::closure::Closure<dyn FnMut(web_sys::BeforeUnloadEvent)>);
#[cfg(target_arch = "wasm32")]
impl UnloadGuard {
    fn new(flag: std::rc::Rc<std::cell::Cell<bool>>) -> Self {
        use wasm_bindgen::JsCast;
        let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
            move |event: web_sys::BeforeUnloadEvent| {
                if flag.get() {
                    event.prevent_default();
                    event.set_return_value("");
                }
            },
        )
            as Box<dyn FnMut(web_sys::BeforeUnloadEvent)>);
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback(
                "beforeunload",
                callback.as_ref().unchecked_ref(),
            );
        }
        Self(callback)
    }
}
#[cfg(target_arch = "wasm32")]
impl Drop for UnloadGuard {
    fn drop(&mut self) {
        use wasm_bindgen::JsCast;
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "beforeunload",
                self.0.as_ref().unchecked_ref(),
            );
        }
    }
}
