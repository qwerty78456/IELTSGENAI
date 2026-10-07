//! Streams finished recordings and voice samples straight from disk.
//!
//! These are plain axum handlers, not server functions: the browser puts the
//! URL in `<audio src>` and in a download link, so the response must be the
//! WAV itself with the right content type and `Range` support (seeking in a
//! 30-minute file), not a server-function envelope. `startup` mounts them at
//! `application::audio::AUDIO_ROUTE` and `application::voices::VOICE_SAMPLE_ROUTE`.
//!
//! Every answer, errors included, says `Cache-Control: private, no-cache`:
//! `private` keeps recordings out of shared caches (Cloudflare), `no-cache`
//! still lets the browser revalidate a large WAV cheaply (304).

use dioxus::server::axum::{
    body::Body,
    extract::{Path, Request},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use tower_http::services::ServeFile;

use crate::domain::Voice;

use super::super::tts::stored_sample;
use super::store::{JobState, JobStore};

/// What every recording and sample response says about caching.
const CACHE_CONTROL: &str = "private, no-cache";

/// `GET /audio/{job_id}`: the WAV of a completed job, or 404 with a
/// teacher-readable reason.
pub async fn serve_audio(path: Path<String>, request: Request) -> Response {
    not_shared(audio(path, request).await)
}

/// `GET /voice-sample/{voice_id}`: the stored sample of a voice, or 404.
/// Nothing is synthesised here; `application::voices` makes samples.
pub async fn serve_voice_sample(path: Path<String>, request: Request) -> Response {
    not_shared(voice_sample(path, request).await)
}

/// Marks a response private and to be revalidated (see the module comment).
fn not_shared(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    response
}

async fn audio(Path(job_id): Path<String>, request: Request) -> Response {
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

async fn voice_sample(Path(voice_id): Path<String>, request: Request) -> Response {
    if Voice::check_id(&voice_id).is_err() {
        return not_found("Voice sample not found");
    }
    let Some(sample) = stored_sample(&voice_id).await else {
        return not_found("Voice sample not found");
    };
    match ServeFile::new(sample.path).try_call(request).await {
        Ok(response) => response.map(Body::new),
        Err(e) => {
            tracing::warn!(voice = %voice_id, "cannot read the voice sample: {e}");
            not_found("The voice sample file is missing")
        }
    }
}

fn not_found(message: &'static str) -> Response {
    (StatusCode::NOT_FOUND, message).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sample_route_refuses_what_is_not_a_voice_id() {
        for id in ["", "..", "../jobs.db", "jobs.db", "en gb", &"x".repeat(101)] {
            let response =
                serve_voice_sample(Path(id.to_string()), Request::new(Body::empty())).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, no-cache",
                "{id}"
            );
        }
    }

    #[test]
    fn every_kind_of_answer_is_kept_out_of_shared_caches() {
        for status in [
            StatusCode::OK,
            StatusCode::PARTIAL_CONTENT,
            StatusCode::NOT_MODIFIED,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let mut response = status.into_response();
            // Whatever a file service said before is replaced.
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            let response = not_shared(response);
            assert_eq!(response.status(), status);
            let values: Vec<_> = response
                .headers()
                .get_all(header::CACHE_CONTROL)
                .iter()
                .collect();
            assert_eq!(values, ["private, no-cache"], "{status}");
        }
    }
}
