//! Background job manager for audio generation
//! Handles long-running audio generation tasks to avoid Cloudflare timeouts
//!
//! Audio files are written to disk (E:\vmq_data\audio\) instead of being held in RAM.
//! A background cleanup task removes completed/failed jobs older than 24 hours.

use serde::{Deserialize, Serialize};
use dioxus::prelude::*;
use crate::domain::{SpeakerConfig, ListeningSection};

#[cfg(feature = "server")]
use once_cell::sync::Lazy;

#[cfg(feature = "server")]
use rusqlite::{params, Connection};

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
    pub created_at_secs: Option<u64>,
}

#[cfg(feature = "server")]
static DB_CONN: Lazy<std::sync::Mutex<rusqlite::Connection>> = Lazy::new(|| {
    let db_path = std::path::PathBuf::from(r"E:\vmq_data\audio_jobs.db");
    // Ensure directory exists
    let _ = std::fs::create_dir_all(db_path.parent().unwrap());
    
    let conn = rusqlite::Connection::open(&db_path).expect("Failed to open SQLite database");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS audio_jobs (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            progress REAL NOT NULL,
            error TEXT,
            audio_path TEXT,
            created_at_secs INTEGER NOT NULL
        )",
        [],
    ).expect("Failed to create audio_jobs table");
    
    std::sync::Mutex::new(conn)
});

#[cfg(feature = "server")]
static CLEANUP_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Ensure the cleanup task is running (idempotent, safe to call multiple times)
#[cfg(feature = "server")]
fn ensure_cleanup_running() {
    CLEANUP_STARTED.get_or_init(|| {
        start_cleanup_task();
    });
}

/// Maximum number of concurrent audio generation jobs
#[cfg(feature = "server")]
const MAX_CONCURRENT_JOBS: usize = 10;

/// Job retention period: 24 hours in seconds
#[cfg(feature = "server")]
const JOB_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Get the audio storage directory path
#[cfg(feature = "server")]
fn audio_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(r"E:\vmq_data\audio")
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
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Helper to lock the database with proper error handling (for server functions)
#[cfg(feature = "server")]
fn lock_db() -> Result<std::sync::MutexGuard<'static, rusqlite::Connection>, ServerFnError> {
    DB_CONN
        .lock()
        .map_err(|e| ServerFnError::new(format!("Internal error: DB lock failed: {}", e)))
}

/// Helper to lock the DB inside spawned tasks (recovers from poisoned mutex)
#[cfg(feature = "server")]
fn lock_db_or_recover() -> std::sync::MutexGuard<'static, rusqlite::Connection> {
    DB_CONN.lock().unwrap_or_else(|e| {
        tracing::warn!("DB mutex was poisoned, recovering: {}", e);
        e.into_inner()
    })
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

    // Check concurrent job limit
    {
        let conn = lock_db()?;
        let active_jobs: i64 = conn.query_row(
            "SELECT count(*) FROM audio_jobs WHERE status IN ('Pending', 'Processing')",
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        
        if active_jobs >= MAX_CONCURRENT_JOBS as i64 {
            return Err(ServerFnError::new(
                "Too many audio jobs in progress. Please wait for existing jobs to complete.",
            ));
        }
    }

    // Generate unique job ID
    let job_id = Uuid::new_v4().to_string();
    let created_at = now_secs();

    // Store job in SQLite
    {
        let conn = lock_db()?;
        conn.execute(
            "INSERT INTO audio_jobs (id, status, progress, error, audio_path, created_at_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![job_id, status_to_string(&JobStatus::Pending), 0.0, None::<String>, None::<String>, created_at],
        ).map_err(|e| ServerFnError::new(format!("Failed to insert job: {}", e)))?;
    }

    // Spawn background task
    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        // Update status to processing
        {
            let conn = lock_db_or_recover();
            let _ = conn.execute(
                "UPDATE audio_jobs SET status = ?1, progress = ?2 WHERE id = ?3",
                params![status_to_string(&JobStatus::Processing), 0.1, job_id_clone],
            );
        }

        // Perform actual audio generation
        match audio_generator::generate_audio(script, speakers, section).await {
            Ok(pcm_data) => {
                // Convert PCM to WAV
                let wav_data = audio_generator::pcm_to_wav(&pcm_data, 24000, 1, 16);

                // Write WAV to disk
                let file_path = audio_dir().join(format!("{}.wav", job_id_clone));
                match std::fs::write(&file_path, &wav_data) {
                    Ok(()) => {
                        tracing::info!(
                            "Audio job {} completed: wrote {} bytes to {:?}",
                            job_id_clone,
                            wav_data.len(),
                            file_path
                        );
                        let conn = lock_db_or_recover();
                        let _ = conn.execute(
                            "UPDATE audio_jobs SET status = ?1, progress = ?2, audio_path = ?3 WHERE id = ?4",
                            params![status_to_string(&JobStatus::Completed), 1.0, file_path.to_string_lossy().to_string(), job_id_clone],
                        );
                    }
                    Err(e) => {
                        tracing::error!("Audio job {} failed to write file: {}", job_id_clone, e);
                        let conn = lock_db_or_recover();
                        let _ = conn.execute(
                            "UPDATE audio_jobs SET status = ?1, error = ?2 WHERE id = ?3",
                            params![status_to_string(&JobStatus::Failed), format!("Failed to save audio file: {}", e), job_id_clone],
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!("Audio job {} generation failed: {}", job_id_clone, e);
                let conn = lock_db_or_recover();
                let _ = conn.execute(
                    "UPDATE audio_jobs SET status = ?1, error = ?2 WHERE id = ?3",
                    params![status_to_string(&JobStatus::Failed), format!("Audio generation failed: {}", e), job_id_clone],
                );
            }
        }
    });

    Ok(job_id)
}

/// Check the status of an audio generation job
#[server]
pub async fn check_audio_job_status(job_id: String) -> Result<AudioJob, ServerFnError> {
    let conn = lock_db()?;

    let mut stmt = conn.prepare("SELECT status, progress, error FROM audio_jobs WHERE id = ?1")
        .map_err(|e| ServerFnError::new(format!("DB error: {}", e)))?;
        
    let result = stmt.query_row(params![job_id], |row| {
        let status_str: String = row.get(0)?;
        let progress: f64 = row.get(1)?;
        let error: Option<String> = row.get(2)?;
        
        Ok(AudioJob {
            id: job_id.clone(),
            status: string_to_status(&status_str),
            progress: progress as f32,
            error,
            audio_path: None,
            created_at_secs: None,
        })
    });

    match result {
        Ok(job) => Ok(job),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(ServerFnError::new("Job not found")),
        Err(e) => Err(ServerFnError::new(format!("DB error: {}", e))),
    }
}

/// Get the audio data from a completed job (reads from disk)
#[server]
pub async fn get_audio_job_result(job_id: String) -> Result<Vec<u8>, ServerFnError> {
    let conn = lock_db()?;

    let mut stmt = conn.prepare("SELECT status, error, audio_path FROM audio_jobs WHERE id = ?1")
        .map_err(|e| ServerFnError::new(format!("DB error: {}", e)))?;
        
    let result = stmt.query_row(params![job_id], |row| {
        let status_str: String = row.get(0)?;
        let error: Option<String> = row.get(1)?;
        let audio_path: Option<String> = row.get(2)?;
        
        Ok((string_to_status(&status_str), error, audio_path))
    });

    match result {
        Ok((status, error, audio_path)) => {
            if status == JobStatus::Completed {
                let path = audio_path.ok_or_else(|| ServerFnError::new("Audio file path not available"))?;
                std::fs::read(&path).map_err(|e| {
                    ServerFnError::new(format!("Failed to read audio file: {}", e))
                })
            } else if status == JobStatus::Failed {
                Err(ServerFnError::new(error.unwrap_or_else(|| "Unknown error".to_string())))
            } else {
                Err(ServerFnError::new("Job not completed yet"))
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(ServerFnError::new("Job not found")),
        Err(e) => Err(ServerFnError::new(format!("DB error: {}", e))),
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
            cleanup_old_jobs();
        }
    });
}

/// Remove old jobs and their audio files from disk
#[cfg(feature = "server")]
fn cleanup_old_jobs() {
    let now = now_secs();
    let conn = lock_db_or_recover();
    
    // Find jobs older than 24h
    let cutoff = now.saturating_sub(JOB_MAX_AGE_SECS);
    
    let mut stmt = match conn.prepare("SELECT id, audio_path FROM audio_jobs WHERE created_at_secs <= ?1") {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to prepare cleanup query: {}", e);
            return;
        }
    };
    
    let rows = match stmt.query_map(params![cutoff], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to execute cleanup query: {}", e);
            return;
        }
    };
    
    let mut removed = 0u32;
    for row_result in rows {
        if let Ok((id, audio_path_opt)) = row_result {
            // Delete the audio file from disk if it exists
            if let Some(path) = audio_path_opt {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!("Failed to delete audio file {}: {}", path, e);
                }
            }
            
            // Delete from DB
            if let Err(e) = conn.execute("DELETE FROM audio_jobs WHERE id = ?1", params![id]) {
                tracing::error!("Failed to delete job {} from DB: {}", id, e);
            } else {
                removed += 1;
            }
        }
    }

    if removed > 0 {
        tracing::info!("Cleanup: removed {} old audio job(s)", removed);
    }
}
