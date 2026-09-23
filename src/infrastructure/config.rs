//! Explicit, fallible startup configuration. Never print dotenv contents or secrets.
use std::{
    collections::HashMap,
    io::{ErrorKind, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{audio::Pcm16, tts::voices::VoiceMappings};

pub const DEFAULT_TEXT_MODEL: &str = "gemini-flash-latest";
pub const DEFAULT_TTS_MODEL: &str = "gemini-2.5-pro-preview-tts";
pub const TTS_MAX_INPUT_TOKENS: usize = 8_192;
const PORTABLE_ENV: &str = "# Listening Exam Generator. Restart after editing.\nIP=127.0.0.1\nPORT=8080\nDATA_DIR=./data\nVOICES_PATH=./voices.json\nGEMINI_API_KEY=your_api_key_here\n# GEMINI_TEXT_MODEL=gemini-flash-latest\n# GEMINI_TTS_MODEL=gemini-2.5-pro-preview-tts\n# MUSIC_PATH=./music.wav\nRUST_LOG=info\n";

/// Read only application settings, reporting invalid encodings without their values.
pub fn environment() -> Result<HashMap<String, String>, String> {
    let mut values = HashMap::new();
    for name in [
        "GEMINI_API_KEY",
        "GEMINI_TEXT_MODEL",
        "GEMINI_TTS_MODEL",
        "DATA_DIR",
        "VOICES_PATH",
        "MUSIC_PATH",
        "IP",
        "PORT",
        "RUST_LOG",
    ] {
        match std::env::var(name) {
            Ok(value) => {
                values.insert(name.into(), value);
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(format!("Environment setting {name} must be valid Unicode"));
            }
        }
    }
    Ok(values)
}

#[derive(Default, Debug)]
pub struct StartupOptions {
    pub portable: bool,
    pub config_dir: Option<PathBuf>,
    pub no_open: bool,
    pub non_interactive: bool,
}

impl StartupOptions {
    pub fn parse(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--portable") => options.portable = true,
                Some("--config-dir") => {
                    options.config_dir = Some(PathBuf::from(
                        args.next().ok_or("--config-dir requires a directory")?,
                    ));
                }
                Some("--no-open") => options.no_open = true,
                Some("--non-interactive") => options.non_interactive = true,
                _ => return Err("Unknown argument. Supported: --portable --config-dir PATH --no-open --non-interactive".into()),
            }
        }
        Ok(options)
    }

    pub fn directory(&self) -> Result<PathBuf, String> {
        let cwd = std::env::current_dir().map_err(|_| "Cannot determine working directory")?;
        if let Some(path) = &self.config_dir {
            return Ok(resolve(&cwd, path));
        }
        if self.portable {
            let executable = std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .map(Ok)
                .unwrap_or_else(std::env::current_exe)
                .map_err(|_| "Cannot determine executable directory")?;
            return executable
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "Cannot determine executable directory".into());
        }
        Ok(cwd)
    }
}

// Intentionally no Debug: the API key must not appear in diagnostics.
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub gemini_api_key: Option<String>,
    pub text_model: String,
    pub tts_model: String,
    pub voices: VoiceMappings,
    pub music_path: Option<PathBuf>,
    pub address: SocketAddr,
    pub log_filter: String,
}

fn resolve(base: &Path, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

/// Exclusive creation prevents overwriting files (including races with another launch).
pub fn create_missing(path: &Path, contents: &[u8]) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot inspect {}: {e}", path.display())),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => file
            .write_all(contents)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("Cannot write {}: {e}", path.display())),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("Cannot create {}: {e}", path.display())),
    }
}

impl AppConfig {
    pub fn load(
        base: &Path,
        portable: bool,
        environment: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let env_path = base.join(".env");
        if portable {
            create_missing(&env_path, PORTABLE_ENV.as_bytes())?;
            // Create both first-run templates even when .env is malformed or the key is unset.
            VoiceMappings::create_missing(&base.join("voices.json"))?;
        }
        let mut values = HashMap::new();
        match std::fs::read(&env_path) {
            Ok(bytes) => {
                std::str::from_utf8(&bytes)
                    .map_err(|_| format!("{}: dotenv file must be UTF-8", env_path.display()))?;
                // Do not use dotenv(): it searches parents and mutates global process state.
                for (index, entry) in dotenvy::from_read_iter(bytes.as_slice()).enumerate() {
                    let (key, value) = entry.map_err(|_| format!(
                        "{}: invalid dotenv syntax near entry {}. Check quoting and KEY=value syntax.",
                        env_path.display(), index + 1
                    ))?;
                    values.insert(key, value);
                }
            }
            Err(e) if !portable && e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Cannot read {}: {e}", env_path.display())),
        }
        values.extend(environment.clone());
        let setting_error = |name: &str, reason: &str| {
            format!("{} / environment: {name} {reason}", env_path.display())
        };
        let value =
            |name: &str, default: &str| values.get(name).cloned().unwrap_or_else(|| default.into());
        let data_dir = value("DATA_DIR", "./data");
        if data_dir.trim().is_empty() {
            return Err(setting_error("DATA_DIR", "must not be empty"));
        }
        let data_dir = resolve(base, data_dir);
        let voices_path = values
            .get("VOICES_PATH")
            .map(|path| resolve(base, path))
            .unwrap_or_else(|| {
                if portable {
                    base.join("voices.json")
                } else {
                    data_dir.join("voices.json")
                }
            });
        if values
            .get("VOICES_PATH")
            .is_some_and(|s| s.trim().is_empty())
        {
            return Err(setting_error("VOICES_PATH", "must not be empty"));
        }
        VoiceMappings::create_missing(&voices_path)?;
        let voices = VoiceMappings::load(&voices_path)?;
        let key = value("GEMINI_API_KEY", "");
        if key.trim().is_empty() || key.trim() == "your_api_key_here" {
            return Err(setting_error(
                "GEMINI_API_KEY",
                "is required. Edit .env, replace your_api_key_here with your key, then restart.",
            ));
        }
        if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(setting_error(
                "GEMINI_API_KEY",
                "must not contain whitespace or control characters",
            ));
        }
        let text_model = value("GEMINI_TEXT_MODEL", DEFAULT_TEXT_MODEL);
        let tts_model = value("GEMINI_TTS_MODEL", DEFAULT_TTS_MODEL);
        for (name, model) in [
            ("GEMINI_TEXT_MODEL", &text_model),
            ("GEMINI_TTS_MODEL", &tts_model),
        ] {
            if model.is_empty()
                || !model
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return Err(setting_error(
                    name,
                    "must be a model ID containing letters, digits, '.', '_' or '-'",
                ));
            }
        }
        let ip = value("IP", "127.0.0.1")
            .parse::<IpAddr>()
            .map_err(|_| setting_error("IP", "must be a valid IPv4 or IPv6 address"))?;
        let port = value("PORT", "8080")
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| setting_error("PORT", "must be an integer from 1 to 65535"))?;
        let log_filter = value("RUST_LOG", "info");
        if log_filter.trim().is_empty()
            || tracing_subscriber::EnvFilter::try_new(&log_filter).is_err()
        {
            return Err(setting_error(
                "RUST_LOG",
                "must be a valid logging filter, such as info",
            ));
        }
        let music_path = values
            .get("MUSIC_PATH")
            .filter(|p| !p.trim().is_empty())
            .map(|p| resolve(base, p));
        if let Some(path) = &music_path {
            let bytes = std::fs::read(path)
                .map_err(|e| format!("Cannot read MUSIC_PATH {}: {e}", path.display()))?;
            let pcm = Pcm16::from_wav(&bytes).map_err(|_| {
                format!(
                    "MUSIC_PATH {} must be a valid mono 16-bit WAV",
                    path.display()
                )
            })?;
            if pcm.sample_rate != 24_000 {
                return Err(format!("MUSIC_PATH {} must use 24000 Hz", path.display()));
            }
        }
        Ok(Self {
            data_dir,
            gemini_api_key: Some(key),
            text_model,
            tts_model,
            voices,
            music_path,
            address: SocketAddr::new(ip, port),
            log_filter,
        })
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

static CONFIG: OnceLock<AppConfig> = OnceLock::new();
pub fn initialize(cfg: AppConfig) -> Result<(), String> {
    CONFIG
        .set(cfg)
        .map_err(|_| "Configuration is already initialized".into())
}
pub fn config() -> &'static AppConfig {
    CONFIG
        .get()
        .expect("configuration initialized before server starts")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn environment() -> HashMap<String, String> {
        HashMap::from([("GEMINI_API_KEY".into(), "test-secret-not-a-real-key".into())])
    }
    #[test]
    fn first_launch_creates_both_files_before_requiring_key() {
        let dir = tempfile::tempdir().unwrap();
        let error = AppConfig::load(dir.path(), true, &HashMap::new())
            .err()
            .unwrap();
        assert!(error.contains("GEMINI_API_KEY"));
        assert!(dir.path().join(".env").is_file());
        assert!(dir.path().join("voices.json").is_file());
    }
    #[test]
    fn missing_files_are_created_independently_and_existing_files_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let original = "PORT=8123\nDATA_DIR=résultats\n";
        std::fs::write(dir.path().join(".env"), original).unwrap();
        let cfg = AppConfig::load(dir.path(), true, &environment()).unwrap();
        assert_eq!(cfg.address.port(), 8123);
        assert_eq!(cfg.data_dir, dir.path().join("résultats"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".env")).unwrap(),
            original
        );
        let voices = std::fs::read(dir.path().join("voices.json")).unwrap();
        std::fs::remove_file(dir.path().join(".env")).unwrap();
        AppConfig::load(dir.path(), true, &environment()).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("voices.json")).unwrap(),
            voices
        );
    }
    #[test]
    fn bad_dotenv_is_rejected_without_disclosing_contents_even_with_override() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "GEMINI_API_KEY=\"secret-never-print",
        )
        .unwrap();
        let error = AppConfig::load(dir.path(), true, &environment())
            .err()
            .unwrap();
        assert!(error.contains("dotenv"));
        assert!(!error.contains("secret-never-print"));
    }
    #[test]
    fn settings_are_validated_and_environment_wins() {
        let dir = tempfile::tempdir().unwrap();
        for (name, value) in [
            ("PORT", "0"),
            ("IP", "bad"),
            ("RUST_LOG", "bad["),
            ("DATA_DIR", ""),
            ("GEMINI_TEXT_MODEL", "bad/model"),
            ("VOICES_PATH", ""),
        ] {
            std::fs::write(dir.path().join(".env"), format!("{name}={value}\n")).unwrap();
            assert!(
                AppConfig::load(dir.path(), true, &environment())
                    .err()
                    .unwrap()
                    .contains(name)
            );
        }
        std::fs::write(dir.path().join(".env"), "PORT=1111").unwrap();
        let mut env = environment();
        env.insert("PORT".into(), "2222".into());
        assert_eq!(
            AppConfig::load(dir.path(), true, &env)
                .unwrap()
                .address
                .port(),
            2222
        );
    }
    #[test]
    fn invalid_json_and_inaccessible_file_fail_without_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        std::fs::write(&path, "{broken").unwrap();
        assert!(
            AppConfig::load(dir.path(), true, &environment())
                .err()
                .unwrap()
                .contains("voices.json")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(AppConfig::load(dir.path(), true, &environment()).is_err());
    }
    #[test]
    fn command_line_errors_are_clear() {
        assert!(StartupOptions::parse(["--config-dir".into()]).is_err());
        let options = StartupOptions::parse([
            "--portable".into(),
            "--no-open".into(),
            "--non-interactive".into(),
        ])
        .unwrap();
        assert!(options.portable && options.no_open && options.non_interactive);
    }

    #[test]
    fn invalid_encoding_is_distinct_from_missing_configuration() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), b"GEMINI_API_KEY=secret\xff").unwrap();
        let error = AppConfig::load(dir.path(), true, &environment())
            .err()
            .unwrap();
        assert!(error.contains("UTF-8"));
        assert!(!error.contains("secret"));
        assert!(dir.path().join("voices.json").is_file());
    }

    #[test]
    fn relative_voice_and_music_paths_use_configuration_directory() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("intro résumé.wav");
        std::fs::write(&music, Pcm16::silence(10, 24_000).to_wav()).unwrap();
        let mut env = environment();
        env.insert("VOICES_PATH".into(), "settings/voices.json".into());
        env.insert("MUSIC_PATH".into(), "intro résumé.wav".into());
        let cfg = AppConfig::load(dir.path(), true, &env).unwrap();
        assert_eq!(cfg.music_path, Some(music.clone()));
        assert!(dir.path().join("settings/voices.json").is_file());
        std::fs::write(&music, Pcm16::silence(10, 44_100).to_wav()).unwrap();
        assert!(
            AppConfig::load(dir.path(), true, &env)
                .err()
                .unwrap()
                .contains("24000")
        );
        std::fs::write(&music, b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00").unwrap();
        assert!(
            AppConfig::load(dir.path(), true, &env)
                .err()
                .unwrap()
                .contains("MUSIC_PATH")
        );
        std::fs::remove_file(music).unwrap();
        assert!(
            AppConfig::load(dir.path(), true, &env)
                .err()
                .unwrap()
                .contains("Cannot read MUSIC_PATH")
        );
    }

    #[test]
    fn nonportable_defaults_keep_data_convention_without_creating_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(dir.path(), false, &environment()).unwrap();
        assert_eq!(cfg.data_dir, dir.path().join("data"));
        assert!(dir.path().join("data/voices.json").is_file());
        assert!(!dir.path().join(".env").exists());
    }

    #[test]
    fn invalid_key_diagnostics_do_not_contain_the_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = environment();
        env.insert("GEMINI_API_KEY".into(), "never-print-this-secret\n".into());
        let error = AppConfig::load(dir.path(), true, &env).err().unwrap();
        assert!(error.contains("GEMINI_API_KEY"));
        assert!(!error.contains("never-print-this-secret"));
    }
}
