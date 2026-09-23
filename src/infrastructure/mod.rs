//! Server-only adapters. Nothing in here is compiled into the browser bundle,
//! and nothing in here defines a `#[server]` function: that is the
//! application layer's job.
#![cfg(feature = "server")]

pub mod audio;
pub mod config;
pub mod jobs;
pub mod llm;
pub mod prompts;
pub mod rate_limiter;
pub mod tts;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Process-wide setup before the Dioxus server starts: configuration, data
/// directories and logging. Async resources (the job store) initialise lazily
/// on first use because `dioxus::launch` owns the Tokio runtime.
pub fn bootstrap() {
    let cfg = config::config();
    for dir in [cfg.audio_dir(), cfg.logs_dir()] {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("WARNING: cannot create {}: {e}", dir.display());
        }
    }

    let file_appender = tracing_appender::rolling::daily(cfg.logs_dir(), "listening-generator.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // Keep the writer thread alive for the whole process.
    Box::leak(Box::new(guard));

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    if cfg.gemini_api_key.is_none() {
        tracing::warn!("GEMINI_API_KEY is not set; generation requests will fail until it is");
    }
    tracing::info!(
        data_dir = %cfg.data_dir.display(),
        text_model = %cfg.text_model,
        tts_model = %cfg.tts_model,
        "configuration loaded"
    );
}
