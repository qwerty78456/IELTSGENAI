use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::domain::{AudioRequest, AudioTrack, ExamAudioRequest};

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

/// Where a finished recording is streamed from (axum path syntax). `main.rs`
/// mounts `infrastructure::jobs::serve_audio` here; the browser builds the
/// same URL with `audio_url`.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub const AUDIO_ROUTE: &str = "/audio/{job_id}";

/// The URL of a finished job's WAV, for `<audio src>` and download links.
pub fn audio_url(job_id: &str) -> String {
    format!("/audio/{job_id}")
}

/// Starts synthesising one part's passage; returns the job id to poll.
#[server]
pub async fn start_part_audio(request: AudioRequest) -> Result<String, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{jobs, rate_limiter};

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
    jobs::spawn_part_audio(job_id.clone(), request);
    Ok(job_id)
}

/// Starts rendering the whole exam recording; returns the job id to poll.
#[server]
pub async fn start_exam_audio(request: ExamAudioRequest) -> Result<String, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{jobs, rate_limiter};

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
    jobs::spawn_exam_audio(job_id.clone(), request);
    Ok(job_id)
}

/// State of a job; once complete, also the `AudioTrack` describing its WAV.
#[server]
pub async fn audio_job_status(job_id: String) -> Result<JobView, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::audio::{SAMPLE_RATE, duration_ms_for_len};
    use crate::infrastructure::jobs::{JobState, JobStore};

    let record = JobStore::global()
        .await
        .get(&job_id)
        .await
        .map_err(user_error)?;
    let record = record.ok_or_else(|| ServerFnError::new("Job not found"))?;
    let status = match record.state {
        JobState::Pending => JobStatus::Pending,
        JobState::Processing => JobStatus::Processing,
        JobState::Completed => JobStatus::Completed,
        JobState::Failed => JobStatus::Failed,
    };
    let track = match (record.state, record.output_path.as_deref()) {
        (JobState::Completed, Some(path)) => {
            let len = tokio::fs::metadata(path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            Some(AudioTrack {
                container: "wav".into(),
                sample_rate: SAMPLE_RATE,
                duration_ms: duration_ms_for_len(len),
                location: record.id.clone(),
            })
        }
        _ => None,
    };
    Ok(JobView {
        id: record.id,
        status,
        progress: record.progress,
        error: record.error,
        track,
    })
}
