//! Saved exams: the whole-exam page keeps its draft on the server so the
//! teacher can close the tab and come back to it, recording included.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AudioTrack, Exam, FormatId, MAX_TOPIC_CHARS};

use super::audio::JobStatus;

/// What the exam page saves and gets back. Still a draft: nothing here is
/// "final", it is just kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedExam {
    pub exam: Exam,
    /// Index-aligned with `exam.parts`: the topic typed for each part, which
    /// only reaches `Passage::topic` once a script exists.
    pub topics: Vec<String>,
    /// The exam recording job this exam keeps alive.
    #[serde(default)]
    pub recording_job: Option<String>,
    /// Filled in by `load_exam` from the job table, never trusted from the browser.
    #[serde(default)]
    pub recording: Option<AudioTrack>,
    /// A script changed after the recording was made.
    #[serde(default)]
    pub recording_stale: bool,
    /// Server-populated.
    #[serde(default)]
    pub created_at_secs: i64,
    #[serde(default)]
    pub updated_at_secs: i64,
}

/// One line of the saved-exams list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExamSummary {
    pub id: Uuid,
    pub title: String,
    pub format: FormatId,
    pub parts_total: u8,
    pub parts_with_script: u8,
    pub parts_complete: u8,
    /// `None` when the exam has no recording job.
    pub recording: Option<JobStatus>,
    pub created_at_secs: i64,
    pub updated_at_secs: i64,
}

/// Largest JSON body a saved exam may have. A full four-part exam is well
/// under 200 KB; the cap only bounds disk use on a server without a login.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub const MAX_SAVED_EXAM_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_THEME_CHARS: usize = 2_000;

/// Checks the shape of a record before it is stored; teacher-readable reasons.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn check(saved: &SavedExam) -> Result<(), String> {
    saved
        .exam
        .format
        .check_consistency()
        .map_err(|e| e.to_string())?;
    if saved.topics.len() != saved.exam.parts.len() {
        return Err("The topics do not match the exam's parts".into());
    }
    if saved
        .topics
        .iter()
        .any(|t| t.chars().count() > MAX_TOPIC_CHARS)
    {
        return Err(format!(
            "A topic is too long; keep it under {MAX_TOPIC_CHARS} characters"
        ));
    }
    if saved.exam.title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "The title is too long; keep it under {MAX_TITLE_CHARS} characters"
        ));
    }
    if saved.exam.theme.chars().count() > MAX_THEME_CHARS {
        return Err(format!(
            "The theme is too long; keep it under {MAX_THEME_CHARS} characters"
        ));
    }
    if let Some(job) = &saved.recording_job
        && (job.is_empty()
            || job.len() > 64
            || !job
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
    {
        return Err("The recording reference is not a job id".into());
    }
    Ok(())
}

#[cfg(feature = "server")]
fn summary_of(row: crate::infrastructure::exams::ExamRow) -> Option<ExamSummary> {
    let id = Uuid::parse_str(&row.id).ok()?;
    let format = FormatId::from_key(&row.format)?;
    let recording = row.recording_job.as_ref().map(|_| {
        row.recording_state
            .map(super::audio::status_of)
            .unwrap_or(JobStatus::Failed)
    });
    Some(ExamSummary {
        id,
        title: row.title,
        format,
        parts_total: row.parts_total.clamp(0, 255) as u8,
        parts_with_script: row.parts_with_script.clamp(0, 255) as u8,
        parts_complete: row.parts_complete.clamp(0, 255) as u8,
        recording,
        created_at_secs: row.created_at_secs,
        updated_at_secs: row.updated_at_secs,
    })
}

#[cfg(feature = "server")]
fn parse_id(id: &str) -> Result<String, ServerFnError> {
    Uuid::parse_str(id)
        .map(|id| id.to_string())
        .map_err(|_| ServerFnError::new("Invalid exam id"))
}

/// Saves (or re-saves) the exam under its id and returns its list entry.
#[server]
pub async fn save_exam(saved: SavedExam) -> Result<ExamSummary, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::exams::ExamStore;

    check(&saved).map_err(ServerFnError::new)?;
    let body = serde_json::to_string(&saved).map_err(user_error)?;
    if body.len() > MAX_SAVED_EXAM_BYTES {
        return Err(ServerFnError::new("This exam is too large to save"));
    }
    let row = ExamStore::global()
        .await
        .save(&saved.exam, saved.recording_job.as_deref(), &body)
        .await
        .map_err(user_error)?;
    summary_of(row).ok_or_else(|| ServerFnError::new("Saved, but the exam cannot be listed"))
}

/// Every saved exam, most recently updated first.
#[server]
pub async fn list_exams() -> Result<Vec<ExamSummary>, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::exams::ExamStore;

    let rows = ExamStore::global().await.list().await.map_err(user_error)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.id.clone();
            let summary = summary_of(row);
            if summary.is_none() {
                tracing::warn!(exam = %id, "saved exam has an unknown format; skipped");
            }
            summary
        })
        .collect())
}

/// The saved exam, with its recording looked up afresh in the job table.
#[server]
pub async fn load_exam(id: String) -> Result<SavedExam, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{exams::ExamStore, jobs::JobStore};

    let id = parse_id(&id)?;
    let (row, body) = ExamStore::global()
        .await
        .get(&id)
        .await
        .map_err(user_error)?
        .ok_or_else(|| ServerFnError::new("This exam is no longer saved"))?;
    let mut saved: SavedExam = serde_json::from_str(&body).map_err(|_| {
        ServerFnError::new("This exam was saved by another version and cannot be opened")
    })?;
    saved.created_at_secs = row.created_at_secs;
    saved.updated_at_secs = row.updated_at_secs;
    saved.recording = match &saved.recording_job {
        Some(job) => match JobStore::global()
            .await
            .get(job)
            .await
            .map_err(user_error)?
        {
            Some(record) => super::audio::track_for(&record).await,
            None => None,
        },
        None => None,
    };
    Ok(saved)
}

/// Deletes the exam and, when nothing else refers to it, its recording.
#[server]
pub async fn delete_exam(id: String) -> Result<(), ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{exams::ExamStore, jobs};

    let id = parse_id(&id)?;
    if let Some(output_path) = ExamStore::global()
        .await
        .delete(&id)
        .await
        .map_err(user_error)?
    {
        jobs::remove_output(&output_path).await;
    }
    Ok(())
}
