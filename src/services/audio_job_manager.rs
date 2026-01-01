//! Background job manager for audio generation
//! Handles long-running audio generation tasks to avoid Cloudflare timeouts

use serde::{Deserialize, Serialize};
use dioxus::prelude::*;
use crate::domain::{SpeakerConfig, ListeningSection};

#[cfg(feature = "server")]
use std::collections::HashMap;

#[cfg(feature = "server")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "server")]
use once_cell::sync::Lazy;

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
pub struct AudioJob {
    pub id: String,
    pub status: JobStatus,
    pub progress: f32,
    pub error: Option<String>,
    #[serde(skip)]
    pub audio_data: Option<Vec<u8>>,
}

#[cfg(feature = "server")]
static JOB_STORE: Lazy<Arc<Mutex<HashMap<String, AudioJob>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Start audio generation as a background task
#[server(StartAudioGeneration)]
pub async fn start_audio_generation(
    script: String,
    speakers: Vec<SpeakerConfig>,
    section: ListeningSection,
) -> Result<String, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use uuid::Uuid;

        // Generate unique job ID
        let job_id = Uuid::new_v4().to_string();

        // Create initial job
        let job = AudioJob {
            id: job_id.clone(),
            status: JobStatus::Pending,
            progress: 0.0,
            error: None,
            audio_data: None,
        };

        // Store job
        {
            let mut store = JOB_STORE.lock().unwrap();
            store.insert(job_id.clone(), job);
        }

        // Spawn background task
        let job_id_clone = job_id.clone();
        tokio::spawn(async move {
            // Update status to processing
            {
                let mut store = JOB_STORE.lock().unwrap();
                if let Some(job) = store.get_mut(&job_id_clone) {
                    job.status = JobStatus::Processing;
                    job.progress = 0.1;
                }
            }

            // Perform actual audio generation
            match audio_generator::generate_audio(script, speakers, section).await {
                Ok(pcm_data) => {
                    // Convert PCM to WAV
                    let wav_data = audio_generator::pcm_to_wav(&pcm_data, 24000, 1, 16);

                    // Update job with result
                    let mut store = JOB_STORE.lock().unwrap();
                    if let Some(job) = store.get_mut(&job_id_clone) {
                        job.status = JobStatus::Completed;
                        job.progress = 1.0;
                        job.audio_data = Some(wav_data);
                    }
                }
                Err(e) => {
                    // Update job with error
                    let mut store = JOB_STORE.lock().unwrap();
                    if let Some(job) = store.get_mut(&job_id_clone) {
                        job.status = JobStatus::Failed;
                        job.error = Some(format!("Audio generation failed: {}", e));
                    }
                }
            }
        });

        Ok(job_id)
    }

    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("This function should only be called on the server"))
}

/// Check the status of an audio generation job
#[server(CheckAudioJobStatus)]
pub async fn check_audio_job_status(job_id: String) -> Result<AudioJob, ServerFnError> {
    #[cfg(feature = "server")]
    {
        let store = JOB_STORE.lock().unwrap();

        match store.get(&job_id) {
            Some(job) => {
                // Return job without audio data (just status)
                Ok(AudioJob {
                    id: job.id.clone(),
                    status: job.status.clone(),
                    progress: job.progress,
                    error: job.error.clone(),
                    audio_data: None,
                })
            }
            None => Err(ServerFnError::new("Job not found"))
        }
    }

    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("This function should only be called on the server"))
}

/// Get the audio data from a completed job
#[server(GetAudioJobResult)]
pub async fn get_audio_job_result(job_id: String) -> Result<Vec<u8>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        let mut store = JOB_STORE.lock().unwrap();

        match store.remove(&job_id) {
            Some(job) => {
                if job.status == JobStatus::Completed {
                    job.audio_data.ok_or_else(|| ServerFnError::new("Audio data not available"))
                } else if job.status == JobStatus::Failed {
                    Err(ServerFnError::new(job.error.unwrap_or_else(|| "Unknown error".to_string())))
                } else {
                    Err(ServerFnError::new("Job not completed yet"))
                }
            }
            None => Err(ServerFnError::new("Job not found"))
        }
    }

    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("This function should only be called on the server"))
}
