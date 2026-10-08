use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AudioRequest, AudioTrack, ExamAudioRequest, SpeakerConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

/// What the browser polls while a recording is being made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobView {
    pub id: String,
    pub status: JobStatus,
    pub progress: f32,
    pub error: Option<String>,
    /// Set once the job is complete: where the WAV lives and how long it is.
    pub track: Option<AudioTrack>,
}

/// Maximum simultaneous synthesis jobs per process.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub const MAX_ACTIVE_JOBS: i64 = 10;

/// Where a finished recording is streamed from (axum path syntax).
/// `infrastructure::startup` mounts `infrastructure::jobs::serve_audio` here;
/// the browser builds the same URL with `audio_url`.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub const AUDIO_ROUTE: &str = "/audio/{job_id}";

/// The URL of a finished job's WAV, for `<audio src>` and download links.
pub fn audio_url(job_id: &str) -> String {
    format!("/audio/{job_id}")
}

/// A part recording that started: the job to poll, and the speakers with
/// the voices the server gave them, for the browser to keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStarted {
    pub job_id: String,
    pub speakers: Vec<SpeakerConfig>,
}

/// An exam recording that started: the job to poll, and every part's
/// speakers with their voices, index-aligned with the request's parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExamAudioStarted {
    pub job_id: String,
    pub parts: Vec<Vec<SpeakerConfig>>,
}

/// Starts synthesising one part's passage. Speakers without a usable voice
/// get one first (`voices::prepare_speakers`). A `fresh` request is a new
/// take: the job reads every chunk again instead of reusing earlier speech
/// (`Reuse::Refresh`). `exam` names the saved exam the spend is booked to
/// (none from the part page).
#[server]
pub async fn start_part_audio(
    request: AudioRequest,
    exam: Option<Uuid>,
) -> Result<AudioStarted, ServerFnError> {
    use crate::application::{user_error, voices::prepare_speakers};
    use crate::infrastructure::{jobs, rate_limiter};

    let speakers = prepare_speakers(request.speakers, &[])
        .await
        .map_err(ServerFnError::new)?;
    let request = AudioRequest {
        speakers,
        ..request
    };
    request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Audio).map_err(ServerFnError::new)?;
    let store = jobs::JobStore::global().await;
    if store.active_count().await.map_err(user_error)? >= MAX_ACTIVE_JOBS {
        return Err(ServerFnError::new(
            "Too many recordings are being made right now. Please wait a minute.",
        ));
    }
    let job_id = store
        .create(jobs::JobKind::PartAudio)
        .await
        .map_err(user_error)?;
    let speakers = request.speakers.clone();
    jobs::spawn_part_audio(job_id.clone(), request, exam.map(|id| id.to_string()));
    Ok(AudioStarted { job_id, speakers })
}

/// Starts rendering the whole exam recording. Every part's speakers get
/// voices first, part by part, each part preferring voices the others do not
/// use (`voices::prepare_exam_speakers`). A part whose request is `fresh` is
/// read again ("New take of this part"); the other parts and the
/// announcements reuse earlier speech. The line-ups sent back are what each
/// part is recorded with (`ExamPart::recorded_for`). `exam` names the saved
/// exam the spend is booked to.
#[server]
pub async fn start_exam_audio(
    request: ExamAudioRequest,
    exam: Option<Uuid>,
) -> Result<ExamAudioStarted, ServerFnError> {
    use crate::application::{user_error, voices::prepare_exam_speakers};
    use crate::infrastructure::{jobs, rate_limiter};

    let mut request = request;
    let line_ups: Vec<Vec<SpeakerConfig>> =
        request.parts.iter().map(|p| p.speakers.clone()).collect();
    let exam_format = request.format.format();
    for (part, prepared) in request
        .parts
        .iter_mut()
        .zip(prepare_exam_speakers(&line_ups).await)
    {
        part.speakers = prepared.map_err(|e| {
            let title = exam_format.part(part.passage.part).map_or_else(
                || format!("Part {}", part.passage.part),
                |spec| spec.title.clone(),
            );
            ServerFnError::new(format!("{title}: {e}"))
        })?;
    }
    request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Audio).map_err(ServerFnError::new)?;
    let store = jobs::JobStore::global().await;
    if store.active_count().await.map_err(user_error)? >= MAX_ACTIVE_JOBS {
        return Err(ServerFnError::new(
            "Too many recordings are being made right now. Please wait a minute.",
        ));
    }
    let job_id = store
        .create(jobs::JobKind::ExamAudio)
        .await
        .map_err(user_error)?;
    let parts = request.parts.iter().map(|p| p.speakers.clone()).collect();
    jobs::spawn_exam_audio(job_id.clone(), request, exam.map(|id| id.to_string()));
    Ok(ExamAudioStarted { job_id, parts })
}

/// The status the browser sees for a stored job.
#[cfg(feature = "server")]
pub(crate) fn status_of(state: crate::infrastructure::jobs::JobState) -> JobStatus {
    use crate::infrastructure::jobs::JobState;
    match state {
        JobState::Pending => JobStatus::Pending,
        JobState::Processing => JobStatus::Processing,
        JobState::Completed => JobStatus::Completed,
        JobState::Failed => JobStatus::Failed,
    }
}

/// The `AudioTrack` of a completed job: its WAV under the current `DATA_DIR`,
/// measured on disk. `None` while the job runs or after it failed.
#[cfg(feature = "server")]
pub(crate) async fn track_for(
    record: &crate::infrastructure::jobs::JobRecord,
) -> Option<AudioTrack> {
    use crate::infrastructure::jobs::JobState;

    if record.state != JobState::Completed {
        return None;
    }
    let path = record.output_file()?;
    track_at(&record.id, &path).await
}

#[cfg(feature = "server")]
async fn track_at(id: &str, path: &std::path::Path) -> Option<AudioTrack> {
    use crate::infrastructure::audio::{SAMPLE_RATE, duration_ms_for_len};
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if !metadata.is_file() {
        return None;
    }
    let len = metadata.len();
    Some(AudioTrack {
        container: "wav".into(),
        sample_rate: SAMPLE_RATE,
        duration_ms: duration_ms_for_len(len),
        location: id.to_string(),
    })
}

/// State of a job; once complete, also the `AudioTrack` describing its WAV.
#[server]
pub async fn audio_job_status(job_id: String) -> Result<JobView, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::jobs::JobStore;

    let record = JobStore::global()
        .await
        .get(&job_id)
        .await
        .map_err(user_error)?;
    let record = record.ok_or_else(|| ServerFnError::new("Job not found"))?;
    let track = track_for(&record).await;
    Ok(JobView {
        id: record.id,
        status: status_of(record.state),
        progress: record.progress,
        error: record.error,
        track,
    })
}

#[cfg(all(test, feature = "server"))]
mod recording_file_tests {
    #[tokio::test]
    async fn missing_wav_and_directories_are_not_playable_tracks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            super::track_at("missing", &dir.path().join("missing.wav"))
                .await
                .is_none()
        );
        assert!(super::track_at("directory", dir.path()).await.is_none());
    }
}
