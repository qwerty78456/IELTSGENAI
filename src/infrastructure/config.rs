//! Process configuration from environment variables (and `.env` in development).

use std::path::PathBuf;
use std::sync::OnceLock;

/// The `-latest` alias is hot-swapped by Google to the newest Flash release
/// (`gemini-3.8-flash` at the time of writing; two weeks' e-mail notice before
/// breaking changes). Deliberately not pinned. `GEMINI_TEXT_MODEL` pins a
/// versioned id if a swap ever breaks the prompts.
pub const DEFAULT_TEXT_MODEL: &str = "gemini-flash-latest";
/// Checked against ai.google.dev on 2026-09-21: still served (paid tier only,
/// 8,192 input tokens, 16,384 output tokens, one or two voices) but listed on
/// the deprecations page with `gemini-3.1-flash-tts-preview` as the successor
/// and no shutdown date. Override with `GEMINI_TTS_MODEL` when it goes.
pub const DEFAULT_TTS_MODEL: &str = "gemini-2.5-pro-preview-tts";
/// Input limit of the default TTS model; longer scripts are read turn by turn.
pub const TTS_MAX_INPUT_TOKENS: usize = 8_192;

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Root for the job database, rendered audio and logs.
    pub data_dir: PathBuf,
    /// `None` when unset: the server starts, generation fails with a readable message.
    pub gemini_api_key: Option<String>,
    pub text_model: String,
    pub tts_model: String,
    /// `voices.json` mapping gender + accent to Gemini voice names.
    pub voices_path: PathBuf,
    /// Optional 24 kHz mono 16-bit WAV played at the start and end of an exam recording.
    pub music_path: Option<PathBuf>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();
        let data_dir = PathBuf::from(env_or("DATA_DIR", "./data"));
        let gemini_api_key = std::env::var("GEMINI_API_KEY")
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty() && k != "your_api_key_here");
        let voices_path = std::env::var("VOICES_PATH")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("voices.json"));
        let music_path = std::env::var("MUSIC_PATH").ok().filter(|v| !v.trim().is_empty()).map(PathBuf::from);
        Self {
            data_dir,
            gemini_api_key,
            text_model: env_or("GEMINI_TEXT_MODEL", DEFAULT_TEXT_MODEL),
            tts_model: env_or("GEMINI_TTS_MODEL", DEFAULT_TTS_MODEL),
            voices_path,
            music_path,
        }
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.data_dir.join("audio")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("jobs.db")
    }
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

/// The process configuration, read once.
pub fn config() -> &'static AppConfig {
    CONFIG.get_or_init(AppConfig::from_env)
}
