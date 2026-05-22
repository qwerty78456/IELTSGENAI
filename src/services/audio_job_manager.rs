//! Background job manager for audio generation
//! Handles long-running audio generation tasks to avoid Cloudflare timeouts
//!
//! Audio files are written to disk (in DATA_DIR/audio/) instead of being held in RAM.
//! A background cleanup task removes completed/failed jobs older than 24 hours.

use serde::{Deserialize, Serialize};
use dioxus::prelude::*;
use crate::domain::{SpeakerConfig, ListeningSection};

#[cfg(feature = "server")]
use super::audio_generator;

/// Job status enum
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

/// Audio generation job
#[derive(Serialize, Deserialize, Clone, Debug)]
#[allow(dead_code)]
pub struct AudioJob {
    pub id: String,
    pub status: JobStatus,
    pub progress: f32,
    pub error: Option<String>,
    /// Path to the WAV file on disk (server only, not serialized over the wire)
    #[serde(skip)]
    pub audio_path: Option<String>,
    /// Unix timestamp (seconds) when the job was created (server only)
    #[serde(skip)]
    pub created_at_secs: Option<i64>,
}

#[cfg(feature = "server")]
static CLEANUP_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

#[cfg(feature = "server")]
static DB_POOL: tokio::sync::OnceCell<sqlx::SqlitePool> = tokio::sync::OnceCell::const_new();

/// Maximum number of concurrent audio generation jobs
#[cfg(feature = "server")]
const MAX_CONCURRENT_JOBS: usize = 10;

/// Job retention period: 24 hours in seconds
#[cfg(feature = "server")]
const JOB_MAX_AGE_SECS: i64 = 24 * 60 * 60;

#[cfg(feature = "server")]
async fn get_db() -> &'static sqlx::SqlitePool {
    DB_POOL.get_or_init(|| async {
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
        let db_path = std::path::PathBuf::from(&data_dir).join("audio_jobs.db");
        // Ensure directory exists
        let _ = std::fs::create_dir_all(db_path.parent().unwrap());
        
        let conn_str = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
        
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&conn_str)
            .await
            .expect("Failed to connect to SQLite database");
            
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS audio_jobs (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                progress REAL NOT NULL,
                error TEXT,
                audio_path TEXT,
                created_at_secs INTEGER NOT NULL
            )"
        )
        .execute(&pool)
        .await
        .expect("Failed to create audio_jobs table");
        
        pool
    }).await
}

/// Ensure the cleanup task is running (idempotent, safe to call multiple times)
#[cfg(feature = "server")]
fn ensure_cleanup_running() {
    CLEANUP_STARTED.get_or_init(|| {
        start_cleanup_task();
    });
}

/// Get the audio storage directory path
#[cfg(feature = "server")]
fn audio_dir() -> std::path::PathBuf {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    std::path::PathBuf::from(data_dir).join("audio")
}

/// Ensure the audio storage directory exists. Call at server startup.
#[cfg(feature = "server")]
pub fn ensure_audio_dir() -> Result<(), String> {
    let dir = audio_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create audio directory {:?}: {}", dir, e))
}

/// Get the current Unix timestamp in seconds
#[cfg(feature = "server")]
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(feature = "server")]
fn status_to_string(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "Pending",
        JobStatus::Processing => "Processing",
        JobStatus::Completed => "Completed",
        JobStatus::Failed => "Failed",
    }
}

#[cfg(feature = "server")]
fn string_to_status(s: &str) -> JobStatus {
    match s {
        "Pending" => JobStatus::Pending,
        "Processing" => JobStatus::Processing,
        "Completed" => JobStatus::Completed,
        "Failed" => JobStatus::Failed,
        _ => JobStatus::Failed,
    }
}

/// Start audio generation as a background task
#[server]
pub async fn start_audio_generation(
    script: String,
    speakers: Vec<SpeakerConfig>,
    section: ListeningSection,
) -> Result<String, ServerFnError> {
    use uuid::Uuid;

    // Ensure background cleanup task is running
    ensure_cleanup_running();

    let pool = get_db().await;

    // Check concurrent job limit
    let active_jobs: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audio_jobs WHERE status IN ('Pending', 'Processing')"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("DB error: {}", e)))?;
        
    if active_jobs.0 >= MAX_CONCURRENT_JOBS as i64 {
        return Err(ServerFnError::new(
            "Too many audio jobs in progress. Please wait for existing jobs to complete.",
        ));
    }

    // Generate unique job ID
    let job_id = Uuid::new_v4().to_string();
    let created_at = now_secs();
    let status_str = status_to_string(&JobStatus::Pending);

    // Store job in SQLite
    sqlx::query(
        "INSERT INTO audio_jobs (id, status, progress, error, audio_path, created_at_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    )
    .bind(&job_id)
    .bind(status_str)
    .bind(0.0f32)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(created_at)
    .execute(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Failed to insert job: {}", e)))?;

    // Spawn background task
    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        let pool = get_db().await;
        
        // Update status to processing
        let _ = sqlx::query("UPDATE audio_jobs SET status = ?1, progress = ?2 WHERE id = ?3")
            .bind(status_to_string(&JobStatus::Processing))
            .bind(0.1f32)
            .bind(&job_id_clone)
            .execute(pool)
            .await;

        // Perform actual audio generation
        match audio_generator::generate_audio(script, speakers, section).await {
            Ok(pcm_data) => {
                // Convert PCM to WAV
                let wav_data = audio_generator::pcm_to_wav(&pcm_data, 24000, 1, 16);

                // Write WAV to disk
                let file_path = audio_dir().join(format!("{}.wav", job_id_clone));
                // Use spawn_blocking for file I/O to avoid blocking the Tokio runtime
                let write_result = tokio::task::spawn_blocking({
                    let path = file_path.clone();
                    let data = wav_data.clone();
                    move || std::fs::write(&path, &data)
                }).await.unwrap();

                match write_result {
                    Ok(()) => {
                        tracing::info!(
                            "Audio job {} completed: wrote {} bytes to {:?}",
                            job_id_clone,
                            wav_data.len(),
                            file_path
                        );
                        let _ = sqlx::query(
                            "UPDATE audio_jobs SET status = ?1, progress = ?2, audio_path = ?3 WHERE id = ?4"
                        )
                        .bind(status_to_string(&JobStatus::Completed))
                        .bind(1.0f32)
                        .bind(file_path.to_string_lossy().to_string())
                        .bind(&job_id_clone)
                        .execute(pool)
                        .await;
                    }
                    Err(e) => {
                        tracing::error!("Audio job {} failed to write file: {}", job_id_clone, e);
                        let _ = sqlx::query(
                            "UPDATE audio_jobs SET status = ?1, error = ?2 WHERE id = ?3"
                        )
                        .bind(status_to_string(&JobStatus::Failed))
                        .bind(format!("Failed to save audio file: {}", e))
                        .bind(&job_id_clone)
                        .execute(pool)
                        .await;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Audio job {} generation failed: {}", job_id_clone, e);
                let _ = sqlx::query(
                    "UPDATE audio_jobs SET status = ?1, error = ?2 WHERE id = ?3"
                )
                .bind(status_to_string(&JobStatus::Failed))
                .bind(format!("Audio generation failed: {}", e))
                .bind(&job_id_clone)
                .execute(pool)
                .await;
            }
        }
    });

    Ok(job_id)
}

/// Check the status of an audio generation job
#[server]
pub async fn check_audio_job_status(job_id: String) -> Result<AudioJob, ServerFnError> {
    let pool = get_db().await;

    let result: Option<(String, f32, Option<String>)> = sqlx::query_as(
        "SELECT status, progress, error FROM audio_jobs WHERE id = ?1"
    )
    .bind(&job_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("DB error: {}", e)))?;

    match result {
        Some((status_str, progress, error)) => Ok(AudioJob {
            id: job_id.clone(),
            status: string_to_status(&status_str),
            progress,
            error,
            audio_path: None,
            created_at_secs: None,
        }),
        None => Err(ServerFnError::new("Job not found")),
    }
}

/// Get the audio data from a completed job (reads from disk)
#[server]
pub async fn get_audio_job_result(job_id: String) -> Result<Vec<u8>, ServerFnError> {
    let pool = get_db().await;

    let result: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT status, error, audio_path FROM audio_jobs WHERE id = ?1"
    )
    .bind(&job_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("DB error: {}", e)))?;

    match result {
        Some((status_str, error, audio_path)) => {
            let status = string_to_status(&status_str);
            if status == JobStatus::Completed {
                let path = audio_path.ok_or_else(|| ServerFnError::new("Audio file path not available"))?;
                
                // Read file via spawn_blocking
                tokio::task::spawn_blocking(move || std::fs::read(&path))
                    .await
                    .unwrap()
                    .map_err(|e| ServerFnError::new(format!("Failed to read audio file: {}", e)))
            } else if status == JobStatus::Failed {
                Err(ServerFnError::new(error.unwrap_or_else(|| "Unknown error".to_string())))
            } else {
                Err(ServerFnError::new("Job not completed yet"))
            }
        }
        None => Err(ServerFnError::new("Job not found")),
    }
}

/// Start the background cleanup task. Call once at server startup.
/// Runs every hour and removes jobs older than 24 hours, deleting their audio files.
#[cfg(feature = "server")]
pub fn start_cleanup_task() {
    tokio::spawn(async {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            cleanup_old_jobs().await;
        }
    });
}

/// Remove old jobs and their audio files from disk
#[cfg(feature = "server")]
async fn cleanup_old_jobs() {
    let now = now_secs();
    let pool = get_db().await;
    
    // Find jobs older than 24h
    let cutoff = now.saturating_sub(JOB_MAX_AGE_SECS);
    
    let rows: Result<Vec<(String, Option<String>)>, _> = sqlx::query_as(
        "SELECT id, audio_path FROM audio_jobs WHERE created_at_secs <= ?1"
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await;
    
    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to execute cleanup query: {}", e);
            return;
        }
    };
    
    let mut removed = 0u32;
    for (id, audio_path_opt) in rows {
        // Delete the audio file from disk if it exists
        if let Some(path) = audio_path_opt {
            // spawn_blocking for file IO
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!("Failed to delete audio file {}: {}", path, e);
                }
            }).await;
        }
        
        // Delete from DB
        if let Err(e) = sqlx::query("DELETE FROM audio_jobs WHERE id = ?1")
            .bind(&id)
            .execute(pool)
            .await 
        {
            tracing::error!("Failed to delete job {} from DB: {}", id, e);
        } else {
            removed += 1;
        }
    }

    if removed > 0 {
        tracing::info!("Cleanup: removed {} old audio job(s)", removed);
    }
}
