//! Job execution on the Tokio runtime plus the hourly clean-up.

use std::sync::OnceLock;

use std::collections::HashMap;

use crate::domain::{AudioProgram, AudioRequest, ExamAudioRequest};

use super::super::audio::{render_program, AudioError, Pcm16, ProgramAssets};
use super::super::config::config;
use super::super::llm::{GeminiClient, LlmError};
use super::super::tts::{synthesize_passage, TtsError};
use super::store::{now_secs, JobStore};

/// Outputs older than this are deleted.
const JOB_MAX_AGE_SECS: i64 = 24 * 60 * 60;
const CLEANUP_INTERVAL_SECS: u64 = 3_600;

static CLEANUP_STARTED: OnceLock<()> = OnceLock::new();

/// Starts the periodic clean-up once per process. Safe to call on every request.
pub fn ensure_cleanup_running() {
    CLEANUP_STARTED.get_or_init(|| {
        tokio::spawn(async {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(CLEANUP_INTERVAL_SECS));
            loop {
                interval.tick().await;
                cleanup_old_jobs().await;
            }
        });
    });
}

async fn cleanup_old_jobs() {
    let store = JobStore::global().await;
    match store.purge_before(now_secs() - JOB_MAX_AGE_SECS).await {
        Ok(paths) => {
            let removed = paths.len();
            for path in paths {
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!("could not delete {path}: {e}");
                    }
                })
                .await;
            }
            if removed > 0 {
                tracing::info!("cleanup removed {removed} old job(s)");
            }
        }
        Err(e) => tracing::error!("cleanup query failed: {e}"),
    }
}

/// Writes a WAV under `DATA_DIR/audio/<job_id>.wav` and returns its path.
async fn store_wav(job_id: &str, pcm: &Pcm16) -> Result<std::path::PathBuf, AudioError> {
    let path = config().audio_dir().join(format!("{job_id}.wav"));
    let wav = pcm.to_wav();
    let target = path.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&target, &wav))
        .await
        .map_err(|e| AudioError::Asset(format!("write task failed: {e}")))?
        .map_err(|e| AudioError::Asset(format!("cannot write {}: {e}", path.display())))?;
    Ok(path)
}

/// Runs `work` as the body of job `job_id`, recording the outcome in the store.
fn run_job<F>(job_id: String, label: &'static str, work: F)
where
    F: std::future::Future<Output = Result<Pcm16, AudioError>> + Send + 'static,
{
    tokio::spawn(async move {
        let store = JobStore::global().await;
        let _ = store.mark_processing(&job_id, 0.1).await;
        let outcome = async {
            let pcm = work.await?;
            let path = store_wav(&job_id, &pcm).await?;
            Ok::<_, AudioError>((path, pcm.duration_ms()))
        }
        .await;
        match outcome {
            Ok((path, duration_ms)) => {
                tracing::info!(job = %job_id, duration_ms, "{label} ready at {}", path.display());
                let _ = store.complete(&job_id, &path.to_string_lossy()).await;
            }
            Err(e) => {
                tracing::error!(job = %job_id, "{label} failed: {e}");
                let _ = store.fail(&job_id, &e.to_string()).await;
            }
        }
    });
}

fn client() -> Result<GeminiClient, AudioError> {
    GeminiClient::from_config().map_err(|e: LlmError| AudioError::Tts(TtsError::Llm(e)))
}

/// Synthesises one passage in the background and stores the WAV under the job id.
pub fn spawn_part_audio(job_id: String, request: AudioRequest) {
    run_job(job_id, "part audio", async move {
        let client = client()?;
        Ok(synthesize_passage(&client, &request.passage, &request.speakers).await?)
    });
}

/// Synthesises every part, then renders the format's programme (announcements,
/// tones, pauses, replays) into one recording.
pub fn spawn_exam_audio(job_id: String, request: ExamAudioRequest) {
    run_job(job_id, "exam audio", async move {
        let client = client()?;
        let assets = ProgramAssets::from_config()?;
        let mut passages: HashMap<u8, Pcm16> = HashMap::new();
        for part in &request.parts {
            let pcm = synthesize_passage(&client, &part.passage, &part.speakers).await?;
            passages.insert(part.passage.part, pcm);
        }
        let program = AudioProgram::for_format(&request.format.format());
        render_program(&program, &passages, &client, &assets).await
    });
}
