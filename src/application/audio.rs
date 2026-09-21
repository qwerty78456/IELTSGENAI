use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::domain::{AudioRequest, ExamAudioRequest};

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
}

/// Maximum simultaneous synthesis jobs per process.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub const MAX_ACTIVE_JOBS: i64 = 10;

/// Starts synthesising one part's passage; returns the job id to poll.
#[server]
pub async fn start_part_audio(request: AudioRequest) -> Result<String, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{jobs, rate_limiter};

    request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::Audio).map_err(ServerFnError::new)?;
    jobs::ensure_cleanup_running();
    let store = jobs::JobStore::global().await;
    if store.active_count().await.map_err(user_error)? >= MAX_ACTIVE_JOBS {
        return Err(ServerFnError::new("Too many recordings are being made right now. Please wait a minute."));
    }
    let job_id = store.create(jobs::JobKind::PartAudio).await.map_err(user_error)?;
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
    jobs::ensure_cleanup_running();
    let store = jobs::JobStore::global().await;
    if store.active_count().await.map_err(user_error)? >= MAX_ACTIVE_JOBS {
        return Err(ServerFnError::new("Too many recordings are being made right now. Please wait a minute."));
    }
    let job_id = store.create(jobs::JobKind::ExamAudio).await.map_err(user_error)?;
    jobs::spawn_exam_audio(job_id.clone(), request);
    Ok(job_id)
}

#[server]
pub async fn audio_job_status(job_id: String) -> Result<JobView, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::jobs::{JobState, JobStore};

    let record = JobStore::global().await.get(&job_id).await.map_err(user_error)?;
    let record = record.ok_or_else(|| ServerFnError::new("Job not found"))?;
    let status = match record.state {
        JobState::Pending => JobStatus::Pending,
        JobState::Processing => JobStatus::Processing,
        JobState::Completed => JobStatus::Completed,
        JobState::Failed => JobStatus::Failed,
    };
    Ok(JobView { id: record.id, status, progress: record.progress, error: record.error })
}

/// The finished WAV bytes of a completed job.
#[server]
pub async fn audio_job_result(job_id: String) -> Result<Vec<u8>, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::jobs::{JobState, JobStore};

    let record = JobStore::global().await.get(&job_id).await.map_err(user_error)?;
    let record = record.ok_or_else(|| ServerFnError::new("Job not found"))?;
    match record.state {
        JobState::Completed => {
            let path = record.output_path.ok_or_else(|| ServerFnError::new("The recording file is missing"))?;
            tokio::task::spawn_blocking(move || std::fs::read(&path))
                .await
                .map_err(user_error)?
                .map_err(|e| ServerFnError::new(format!("Cannot read the recording: {e}")))
        }
        JobState::Failed => Err(ServerFnError::new(record.error.unwrap_or_else(|| "The recording failed".into()))),
        _ => Err(ServerFnError::new("The recording is not finished yet")),
    }
}
