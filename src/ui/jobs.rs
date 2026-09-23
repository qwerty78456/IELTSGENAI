//! Waiting on server-side audio jobs from the browser.
//!
//! A recording outlives an HTTP request, so a view starts the job and then
//! polls `audio_job_status` at a cadence that suits the job kind. Giving up
//! never cancels the job: the id stays with the caller so the teacher can
//! check again later.

use crate::application::audio::{JobStatus, JobView, audio_job_status};

/// Cadence and ceiling for one part's recording. A three-voice part is read
/// turn by turn (30-50 synthesis requests), so five minutes was too short.
pub const PART_AUDIO_POLL_MS: u32 = 2_000;
pub const PART_AUDIO_DEADLINE_MS: u32 = 15 * 60_000;
/// Cadence and ceiling for the whole exam recording: every part is read,
/// then announcements, pauses and replays are assembled.
pub const EXAM_AUDIO_POLL_MS: u32 = 5_000;
pub const EXAM_AUDIO_DEADLINE_MS: u32 = 45 * 60_000;

/// Polls `job_id` until it completes or fails, or until `deadline_ms` has
/// passed. `on_progress` receives the server's progress figure after every poll.
pub async fn wait_for_job(
    job_id: &str,
    poll_ms: u32,
    deadline_ms: u32,
    mut on_progress: impl FnMut(f32),
) -> Result<JobView, String> {
    let mut waited_ms: u32 = 0;
    while waited_ms < deadline_ms {
        sleep_ms(poll_ms).await;
        waited_ms = waited_ms.saturating_add(poll_ms);
        let job = audio_job_status(job_id.to_string())
            .await
            .map_err(|e| format!("Could not check the recording: {e}"))?;
        on_progress(job.progress);
        match job.status {
            JobStatus::Completed => return Ok(job),
            JobStatus::Failed => {
                return Err(job
                    .error
                    .unwrap_or_else(|| "The recording failed".to_string()));
            }
            JobStatus::Pending | JobStatus::Processing => {}
        }
    }
    Err(format!(
        "The recording is still running after {} minutes. Use \"Check again\" in a while.",
        deadline_ms / 60_000
    ))
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep_ms(ms: u32) {
    gloo_timers::future::TimeoutFuture::new(ms).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep_ms(ms: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}
