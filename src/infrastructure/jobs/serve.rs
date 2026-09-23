//! Streams finished recordings straight from disk.
//!
//! This is a plain axum handler, not a server function: the browser puts the
//! URL in `<audio src>` and in a download link, so the response must be the
//! WAV itself with the right content type and `Range` support (seeking in a
//! 30-minute file), not a server-function envelope. `main.rs` mounts it at
//! `application::audio::AUDIO_ROUTE`.

use dioxus::server::axum::{
    body::Body,
    extract::{Path, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower_http::services::ServeFile;

use super::store::{JobState, JobStore};

/// `GET /audio/{job_id}`: the WAV of a completed job, or 404 with a
/// teacher-readable reason.
pub async fn serve_audio(Path(job_id): Path<String>, request: Request) -> Response {
    let record = match JobStore::global().await.get(&job_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return not_found("Recording not found"),
        Err(e) => {
            tracing::error!(job = %job_id, "cannot look up the recording: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Cannot look up the recording",
            )
                .into_response();
        }
    };
    let path = match (record.state, record.output_file()) {
        (JobState::Completed, Some(path)) => path,
        _ => return not_found("Recording not found or not finished yet"),
    };
    match ServeFile::new(path).try_call(request).await {
        Ok(response) => response.map(Body::new),
        Err(e) => {
            tracing::warn!(job = %job_id, "cannot read the recording file: {e}");
            not_found("The recording file is missing")
        }
    }
}

fn not_found(message: &'static str) -> Response {
    (StatusCode::NOT_FOUND, message).into_response()
}
