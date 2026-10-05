//! Job execution on the Tokio runtime plus the hourly clean-up.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;

use crate::domain::{AudioProgram, AudioRequest, ExamAudioRequest, UsageStep};

use super::super::audio::{AudioError, Pcm16, ProgramAssets, render_program};
use super::super::config::config;
use super::super::llm::{GeminiClient, LlmError};
use super::super::tts::{
    Reuse, SAMPLE_KEEP_HOURS, TtsError, purge_speech_cache, synthesize_passage,
};
use super::store::{JobStore, now_secs, output_file_in};

const CLEANUP_INTERVAL_SECS: u64 = 3_600;

static CLEANUP_STARTED: OnceLock<()> = OnceLock::new();

/// Starts the periodic clean-up once per process. Safe to call on every
/// request. Recordings a saved exam refers to are never purged; deleting the
/// exam removes them. Voice samples go after `SAMPLE_KEEP_HOURS` unused, so
/// the loop runs even when `AUDIO_RETENTION_HOURS` and `SPEECH_CACHE_HOURS`
/// are both 0: a sample made later still needs it.
pub fn ensure_cleanup_running() {
    CLEANUP_STARTED.get_or_init(|| {
        let recordings = config().audio_retention_secs();
        let speech_cache = config().speech_cache_secs();
        match recordings {
            Some(_) => tracing::info!(
                "unsaved recordings are purged after {} hour(s)",
                config().audio_retention_hours
            ),
            None => tracing::info!(
                "AUDIO_RETENTION_HOURS=0: recordings are kept until their exam is deleted"
            ),
        }
        match speech_cache {
            Some(_) => tracing::info!(
                "synthesised speech is reused for {} hour(s) after its last use",
                config().speech_cache_hours
            ),
            None => tracing::info!("SPEECH_CACHE_HOURS=0: synthesised speech is not reused"),
        }
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(CLEANUP_INTERVAL_SECS));
            loop {
                interval.tick().await;
                if let Some(max_age_secs) = recordings {
                    cleanup_old_jobs(max_age_secs).await;
                }
                if let Some(max_age_secs) = speech_cache {
                    cleanup_speech_cache(max_age_secs).await;
                }
                cleanup_voice_samples().await;
            }
        });
    });
}

async fn cleanup_speech_cache(max_age_secs: i64) {
    let directory = config().speech_cache_dir();
    let max_age = std::time::Duration::from_secs(max_age_secs.max(0) as u64);
    let removed =
        tokio::task::spawn_blocking(move || purge_speech_cache(&directory, max_age)).await;
    if let Ok(removed @ 1..) = removed {
        tracing::info!("cleanup removed {removed} unused speech chunk(s)");
    }
}

async fn cleanup_voice_samples() {
    let directory = config().voice_sample_dir();
    let max_age = std::time::Duration::from_secs(SAMPLE_KEEP_HOURS * 3_600);
    let removed =
        tokio::task::spawn_blocking(move || purge_speech_cache(&directory, max_age)).await;
    if let Ok(removed @ 1..) = removed {
        tracing::info!("cleanup removed {removed} unused voice sample(s)");
    }
}

async fn cleanup_old_jobs(max_age_secs: i64) {
    let store = JobStore::global().await;
    match store.purge_before(now_secs() - max_age_secs).await {
        Ok(paths) => {
            let removed = paths.len();
            for path in paths {
                remove_output(&path).await;
            }
            if removed > 0 {
                tracing::info!("cleanup removed {removed} old job(s)");
            }
        }
        Err(e) => tracing::error!("cleanup query failed: {e}"),
    }
}

/// Deletes the WAV a job row pointed at (best effort, logged).
pub async fn remove_output(output_path: &str) {
    let path = output_file_in(&config().audio_dir(), output_path);
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("could not delete {}: {e}", path.display());
        }
    })
    .await;
}

/// Writes a WAV under `DATA_DIR/audio/<job_id>.wav` and returns its file name,
/// which is what the job row stores (see `output_file_in`).
async fn store_wav(job_id: &str, pcm: &Pcm16) -> Result<String, AudioError> {
    let name = format!("{job_id}.wav");
    let path = config().audio_dir().join(&name);
    let wav = pcm.to_wav();
    let target = path.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&target, &wav))
        .await
        .map_err(|e| AudioError::Asset(format!("write task failed: {e}")))?
        .map_err(|e| AudioError::Asset(format!("cannot write {}: {e}", path.display())))?;
    Ok(name)
}

/// Runs `work` as the body of job `job_id`, recording the outcome in the
/// store and what Gemini billed in the usage ledger, failed jobs included:
/// the chunks they finished were paid for.
fn run_job<W, F>(job_id: String, label: &'static str, exam: Option<String>, work: W)
where
    W: FnOnce(GeminiClient) -> F + Send + 'static,
    F: std::future::Future<Output = Result<Pcm16, AudioError>> + Send + 'static,
{
    tokio::spawn(async move {
        let store = JobStore::global().await;
        let _ = store.mark_processing(&job_id, 0.1).await;
        let client = match client() {
            Ok(client) => client,
            Err(e) => {
                tracing::error!(job = %job_id, "{label} failed: {e}");
                let _ = store.fail(&job_id, &e.to_string()).await;
                return;
            }
        };
        let outcome = async {
            let pcm = work(client.clone()).await?;
            let name = store_wav(&job_id, &pcm).await?;
            Ok::<_, AudioError>((name, pcm.duration_ms()))
        }
        .await;
        let usage = client.usage();
        tracing::info!(
            job = %job_id,
            requests = usage.requests,
            reused = usage.reused,
            output_tokens = usage.output_tokens,
            micro_usd = usage.micro_usd,
            "{label} spend"
        );
        crate::infrastructure::usage::record(
            UsageStep::Recording,
            exam.as_deref(),
            client.tts_model(),
            &usage,
        )
        .await;
        match outcome {
            Ok((name, duration_ms)) => {
                tracing::info!(
                    job = %job_id, duration_ms,
                    "{label} ready at {}", config().audio_dir().join(&name).display()
                );
                let _ = store.complete(&job_id, &name).await;
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

/// Whether a part's earlier speech may be reused: not when the teacher asked
/// for a new take of it (`AudioRequest::fresh`). Announcements are not a part
/// and always reuse.
fn reuse_for(job_id: &str, part: &AudioRequest) -> Reuse {
    if part.fresh {
        tracing::info!(
            job = %job_id,
            part = part.passage.part,
            "new take: the part's earlier speech is read again"
        );
    }
    Reuse::new_take(part.fresh)
}

/// Synthesises one passage in the background and stores the WAV under the job id.
pub fn spawn_part_audio(job_id: String, request: AudioRequest, exam: Option<String>) {
    let reuse = reuse_for(&job_id, &request);
    run_job(job_id, "part audio", exam, move |client| async move {
        Ok(synthesize_passage(&client, &request.passage, &request.speakers, reuse).await?)
    });
}

/// How many parts one exam job synthesises at the same time. Two: each part
/// is several chunk requests, and `GeminiClient` lets only a few speech
/// requests run at once anyway.
const PARALLEL_PARTS: usize = 2;

/// Synthesises every part (a few at a time), then renders the format's
/// programme (announcements, tones, pauses, replays) into one recording.
/// Progress moves from 0.1 to 0.8 as parts finish; the rest is assembly.
pub fn spawn_exam_audio(job_id: String, request: ExamAudioRequest, exam: Option<String>) {
    let id = job_id.clone();
    run_job(job_id, "exam audio", exam, move |client| async move {
        let assets = ProgramAssets::from_config()?;
        let total = request.parts.len().max(1);
        let done = Arc::new(AtomicUsize::new(0));
        let limit = Arc::new(Semaphore::new(PARALLEL_PARTS));
        let renders = request.parts.iter().map(|part| {
            let client = client.clone();
            let limit = limit.clone();
            let done = done.clone();
            let id = id.clone();
            let reuse = reuse_for(&id, part);
            async move {
                let _permit = limit
                    .acquire()
                    .await
                    .map_err(|e| AudioError::Asset(e.to_string()))?;
                let pcm = synthesize_passage(&client, &part.passage, &part.speakers, reuse).await?;
                let finished = done.fetch_add(1, Ordering::SeqCst) + 1;
                let progress = 0.1 + 0.7 * finished as f32 / total as f32;
                let _ = JobStore::global()
                    .await
                    .mark_processing(&id, progress)
                    .await;
                Ok::<_, AudioError>((part.passage.part, pcm))
            }
        });
        let passages: HashMap<u8, Pcm16> = futures_util::future::try_join_all(renders)
            .await?
            .into_iter()
            .collect();
        let program = AudioProgram::for_format(&request.format.format());
        render_program(&program, &passages, &client, &assets).await
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FormatId, Passage};

    #[test]
    fn a_new_take_refreshes_only_its_part() {
        let part = |number: u8, fresh: bool| AudioRequest {
            passage: Passage::parse(number, "topic", "Speaker A: Hello.", &["Speaker A".into()])
                .unwrap(),
            speakers: Vec::new(),
            fresh,
        };
        let request = ExamAudioRequest {
            format: FormatId::HsgNational,
            parts: vec![part(1, false), part(2, true), part(3, false)],
        };
        let reuse: Vec<Reuse> = request.parts.iter().map(|p| reuse_for("job", p)).collect();
        assert_eq!(reuse, [Reuse::Allow, Reuse::Refresh, Reuse::Allow]);
    }
}
