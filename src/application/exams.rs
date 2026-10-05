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
    /// New scripts are expressive (`PassageRequest::expressive`). Exams saved
    /// before 0.8 load as plain; the exam page starts new exams expressive.
    #[serde(default)]
    pub expressive: bool,
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
    for part in &saved.exam.parts {
        check_speakers(part)?;
    }
    Ok(())
}

/// The shape of a part's speakers: the right count, distinct labels, voice
/// ids that are ids. Two speakers on one voice are kept as they are: the
/// recording refuses them, but a save never loses the teacher's work.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn check_speakers(part: &crate::domain::ExamPart) -> Result<(), String> {
    use crate::domain::{Voice, first_error, validate_speakers};

    let issues = validate_speakers(&part.spec, &part.speakers);
    if let Some(error) = first_error(&issues) {
        let message = &error.message;
        return Err(if message.starts_with(part.spec.title.as_str()) {
            message.clone()
        } else {
            format!("{}: {message}", part.spec.title)
        });
    }
    let line_ups = [
        part.speakers.as_slice(),
        part.recorded_for.as_slice(),
        part.passage
            .as_ref()
            .map_or(&[][..], |p| p.written_for.as_slice()),
    ];
    if line_ups
        .iter()
        .flat_map(|speakers| speakers.iter())
        .filter_map(|s| s.voice_id())
        .any(|id| Voice::check_id(id).is_err())
    {
        return Err(format!(
            "{}: a speaker's voice is not a voice id",
            part.spec.title
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Accent, Gender, VoiceChoice};

    /// A body as 0.7.1 saved it: HSG with Part 1 recorded by three Female
    /// British speakers, written before speakers had voices.
    const SAVED_BY_0_7_1: &str = include_str!("fixtures/saved_exam_0_7_1.json");

    #[test]
    fn a_0_7_1_saved_exam_still_loads() {
        assert!(!SAVED_BY_0_7_1.contains("voice"));
        let saved: SavedExam = serde_json::from_str(SAVED_BY_0_7_1).unwrap();
        check(&saved).unwrap();

        let exam = &saved.exam;
        assert_eq!(exam.format.id, FormatId::HsgNational);
        let part1 = &exam.parts[0];
        assert_eq!(part1.speakers.len(), 3);
        assert!(
            part1
                .speakers
                .iter()
                .all(|s| (s.gender, s.accent) == (Gender::Female, Accent::British))
        );
        assert_eq!(part1.passage.as_ref().unwrap().lines.len(), 6);
        assert_eq!(part1.tasks[0].items.len(), 5);
        assert!(part1.audio.is_some());
        assert!(saved.recording_stale);
        assert!(!saved.expressive);

        let every_speaker = exam.parts.iter().flat_map(|p| p.speakers.iter()).chain(
            exam.format
                .parts
                .iter()
                .flat_map(|p| p.default_speakers.iter()),
        );
        for speaker in every_speaker {
            assert_eq!(speaker.voice, VoiceChoice::Auto, "{}", speaker.label);
        }

        // Saved again by this version, it reads back the same.
        let again: SavedExam =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(again, saved);
    }

    #[test]
    fn opening_a_0_7_exam_marks_nothing_stale() {
        use crate::domain::{Exam, assign_exam_voices, validate_passage};
        use crate::infrastructure::tts::voices::VoiceCatalog;

        let saved: SavedExam = serde_json::from_str(SAVED_BY_0_7_1).unwrap();
        // What the exam page shows for a part: its passage issues and
        // whether a speaker changed since the script or recording.
        let shown = |exam: &Exam| {
            exam.parts
                .iter()
                .map(|part| {
                    let passage = part.passage.as_ref();
                    (
                        passage.map(|p| p.speakers_changed(&part.speakers)),
                        passage.map(|p| validate_passage(p, &part.spec, &part.speakers)),
                        part.recording_stale(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let opened = shown(&saved.exam);
        for (i, part) in saved.exam.parts.iter().enumerate() {
            assert!(part.recorded_for.is_empty());
            assert!(!opened[i].2, "{}", part.spec.title);
            if let Some(change) = opened[i].0 {
                assert_eq!(change, crate::domain::SpeakerChange::default());
            }
        }
        let part1 = &saved.exam.parts[0];
        assert!(part1.passage.as_ref().unwrap().written_for.is_empty());

        // The catalogue arrives and every speaker on Auto gets a voice, as
        // the exam page does on open: still nothing is stale or warned about.
        let catalog = VoiceCatalog::builtin();
        let line_ups: Vec<_> = saved
            .exam
            .parts
            .iter()
            .map(|p| p.speakers.clone())
            .collect();
        let mut voiced = saved.exam.clone();
        for (part, assigned) in voiced
            .parts
            .iter_mut()
            .zip(assign_exam_voices(&line_ups, catalog.voices()))
        {
            assert!(assigned.unvoiced.is_empty());
            part.speakers = assigned.speakers;
        }
        assert!(
            voiced
                .parts
                .iter()
                .flat_map(|p| p.speakers.iter())
                .all(|s| matches!(s.voice, VoiceChoice::Assigned(_)))
        );
        assert_eq!(shown(&voiced), opened);
    }

    #[test]
    fn saving_keeps_shared_voices_but_checks_the_shape() {
        use crate::domain::{Voice, VoiceSource};

        let mut saved: SavedExam = serde_json::from_str(SAVED_BY_0_7_1).unwrap();
        let shared = Voice {
            id: "en-gb-shared-1".into(),
            name: "Shared".into(),
            gender: Gender::Female,
            accent: Accent::British,
            source: VoiceSource::Library,
            description: String::new(),
        };
        // Part 1's three Female British speakers on one voice: saved anyway.
        let part1 = &mut saved.exam.parts[0];
        part1.speakers = part1
            .speakers
            .iter()
            .map(|s| s.clone().with_voice(shared.clone()))
            .collect();
        part1.recorded_for = part1.speakers.clone();
        assert_eq!(check(&saved), Ok(()));

        let mut forged = saved.clone();
        if let VoiceChoice::Chosen(voice) = &mut forged.exam.parts[0].speakers[1].voice {
            voice.id = "../../etc/passwd".into();
        }
        let error = check(&forged).unwrap_err();
        assert!(error.contains("not a voice id"), "{error}");

        let mut short = saved.clone();
        short.exam.parts[0].speakers.pop();
        let error = check(&short).unwrap_err();
        assert!(error.starts_with("Part 1 needs exactly 3"), "{error}");

        let mut twins = saved.clone();
        twins.exam.parts[0].speakers[1].label = "Speaker A".into();
        let error = check(&twins).unwrap_err();
        assert!(error.starts_with("Part 1: Duplicate"), "{error}");
    }
}
