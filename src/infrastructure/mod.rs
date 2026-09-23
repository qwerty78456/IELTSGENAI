//! Server-only adapters and process startup.
#![cfg(feature = "server")]

pub mod audio;
pub mod config;
pub mod exams;
pub mod jobs;
pub mod llm;
pub mod prompts;
pub mod rate_limiter;
pub mod startup;
pub mod tts;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn bootstrap(
    options: &config::StartupOptions,
) -> Result<tracing_appender::non_blocking::WorkerGuard, String> {
    let base = options.directory()?;
    let cfg = config::AppConfig::load(&base, options.portable, &config::environment()?)?;
    for dir in [cfg.audio_dir(), cfg.logs_dir()] {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
        tempfile::NamedTempFile::new_in(&dir)
            .map_err(|e| format!("Cannot write {}: {e}", dir.display()))?;
    }
    // SAFETY: called on the main thread before logger/runtime threads exist.
    // Dioxus's development launcher reads these two variables.
    unsafe {
        std::env::set_var("IP", cfg.address.ip().to_string());
        std::env::set_var("PORT", cfg.address.port().to_string());
    }
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("listening-generator.log")
        .build(cfg.logs_dir())
        .map_err(|e| {
            format!(
                "Cannot initialize logs at {}: {e}",
                cfg.logs_dir().display()
            )
        })?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_new(&cfg.log_filter)
                .map_err(|_| "Invalid RUST_LOG")?,
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .try_init()
        .map_err(|_| "Cannot initialize logging")?;
    config::initialize(cfg)?;
    println!("Configuration: {}", base.display());
    Ok(guard)
}
