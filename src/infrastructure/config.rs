//! Explicit, fallible startup configuration. Never print dotenv contents or secrets.
use std::{
    collections::HashMap,
    io::{ErrorKind, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{audio::Pcm16, tts::voices::VoiceMappings};

pub const DEFAULT_TEXT_MODEL: &str = "gemini-3.8-flash";
pub const DEFAULT_TTS_MODEL: &str = "gemini-3.8-flash-tts";
pub const DEFAULT_THINKING_LEVEL: &str = "low";
pub const DEFAULT_EXAM_BUDGET_USD: &str = "0.70";
pub const DEFAULT_SPEECH_CACHE_HOURS: u32 = 72;
pub const TTS_MAX_INPUT_TOKENS: usize = 8_192;
pub const DEFAULT_AUDIO_RETENTION_HOURS: u32 = 24;
/// The value `.env.example` and the portable template ship with; it means "no key".
const PLACEHOLDER_KEY: &str = "your_api_key_here";
const PORTABLE_ENV: &str = "# Listening Exam Generator. Restart after editing.\nIP=127.0.0.1\nPORT=8080\nDATA_DIR=./data\nVOICES_PATH=./voices.json\n# Leave the key out to use the GEMINI_API_KEY environment variable or to enter it in the browser.\nGEMINI_API_KEY=your_api_key_here\n# GEMINI_TEXT_MODEL=gemini-3.8-flash\n# GEMINI_TTS_MODEL=gemini-3.8-flash-tts\n# GEMINI_THINKING_LEVEL=low\n# EXAM_BUDGET_USD=0.70\n# SPEECH_CACHE_HOURS=72\n# MUSIC_PATH=./music.wav\n# AUDIO_RETENTION_HOURS=24\nRUST_LOG=info\n";

/// Every setting the server reads, from the process environment, the persisted
/// Windows environment and `.env`.
const SETTINGS: [&str; 13] = [
    "GEMINI_API_KEY",
    "GEMINI_TEXT_MODEL",
    "GEMINI_TTS_MODEL",
    "GEMINI_THINKING_LEVEL",
    "EXAM_BUDGET_USD",
    "SPEECH_CACHE_HOURS",
    "DATA_DIR",
    "VOICES_PATH",
    "MUSIC_PATH",
    "AUDIO_RETENTION_HOURS",
    "IP",
    "PORT",
    "RUST_LOG",
];

/// Where the configured Gemini API key came from. Logged at startup; the key never is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrigin {
    /// The server process's own environment.
    Process,
    /// The user or machine environment stored in the Windows registry.
    Windows,
    /// The `.env` file in the configuration directory.
    DotEnv,
}

impl KeyOrigin {
    /// Where the key lives, for messages to the teacher and the startup log.
    pub fn describe(self) -> &'static str {
        match self {
            KeyOrigin::Process => "the server's environment",
            KeyOrigin::Windows => "the Windows environment",
            KeyOrigin::DotEnv => ".env",
        }
    }
}

/// Settings persisted in the Windows registry, the user's before the machine's.
/// A process started before `setx` or the "Environment Variables" dialog ran
/// (an open terminal, an IDE, a launcher) does not inherit them, so they are
/// read directly. Empty on other platforms and when the registry is unreadable.
pub fn windows_environment() -> HashMap<String, String> {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let scopes = [
            (HKEY_CURRENT_USER, "Environment"),
            (
                HKEY_LOCAL_MACHINE,
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            ),
        ];
        let mut values = HashMap::new();
        for (root, path) in scopes {
            let Ok(key) = RegKey::predef(root).open_subkey(path) else {
                continue;
            };
            for name in SETTINGS {
                if values.contains_key(name) {
                    continue;
                }
                if let Ok(value) = key.get_value::<String, _>(name) {
                    values.insert(name.to_string(), value);
                }
            }
        }
        values
    }
    #[cfg(not(windows))]
    {
        HashMap::new()
    }
}

/// Read only application settings, reporting invalid encodings without their values.
pub fn environment() -> Result<HashMap<String, String>, String> {
    let mut values = HashMap::new();
    for name in SETTINGS {
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
    /// `None` when no source has a key; the browser may then supply one
    /// (`infrastructure::secrets`).
    pub gemini_api_key: Option<String>,
    pub key_origin: Option<KeyOrigin>,
    /// The `.env` this configuration was read from (it may not exist).
    pub dotenv_path: PathBuf,
    pub text_model: String,
    pub tts_model: String,
    /// `generation_config.thinking_level` for text requests: low, medium or high.
    pub thinking_level: String,
    /// What one exam should cost at most, in µUSD; 0 means no budget. Only warned about.
    pub exam_budget_micro_usd: u64,
    /// How long a synthesised chunk is kept for reuse; 0 disables the speech cache.
    pub speech_cache_hours: u32,
    pub voices: VoiceMappings,
    pub music_path: Option<PathBuf>,
    /// How long a recording no saved exam refers to is kept; 0 keeps every recording.
    pub audio_retention_hours: u32,
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

/// Parses `.env` without touching the process environment. A missing file is
/// empty, except in portable mode where the template was just created.
fn read_dotenv(env_path: &Path, portable: bool) -> Result<HashMap<String, String>, String> {
    let mut values = HashMap::new();
    match std::fs::read(env_path) {
        Ok(bytes) => {
            std::str::from_utf8(&bytes)
                .map_err(|_| format!("{}: dotenv file must be UTF-8", env_path.display()))?;
            // Do not use dotenv(): it searches parents and mutates global process state.
            for (index, entry) in dotenvy::from_read_iter(bytes.as_slice()).enumerate() {
                let (key, value) = entry.map_err(|_| {
                    format!(
                        "{}: invalid dotenv syntax near entry {}. Check quoting and KEY=value syntax.",
                        env_path.display(),
                        index + 1
                    )
                })?;
                values.insert(key, value);
            }
        }
        Err(e) if !portable && e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot read {}: {e}", env_path.display())),
    }
    Ok(values)
}

/// Why a key typed in the browser cannot be used, if it cannot. Stricter than
/// the startup check: Google API keys are letters, digits, `-` and `_`, and
/// anything else could break the `.env` line it may be written to.
pub fn browser_key_problem(key: &str) -> Option<&'static str> {
    if key.is_empty() {
        return Some("Paste your Gemini API key first.");
    }
    if key == PLACEHOLDER_KEY {
        return Some("That is the placeholder from .env.example, not a key.");
    }
    if !(20..=200).contains(&key.len()) {
        return Some("That does not look like a Gemini API key (wrong length).");
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Some(
            "That does not look like a Gemini API key: only letters, digits, '-', '_' and '.' are allowed.",
        );
    }
    None
}

/// Sets `name=value` in the dotenv file at `path`, keeping every other line.
/// The first `name=` (or `export name=`) line is replaced; otherwise the
/// setting is appended. The new file is written beside the old one and renamed
/// over it, so a crash never leaves half a file. Unix permissions are 0600.
/// `value` must already be safe unquoted (see `browser_key_problem`).
pub fn remember_in_dotenv(path: &Path, name: &str, value: &str) -> Result<(), String> {
    let existing = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("{}: dotenv file must be UTF-8", path.display()))?,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    let assignment = format!("{name}={value}");
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        let setting = line.trim_start();
        let setting = setting
            .strip_prefix("export ")
            .unwrap_or(setting)
            .trim_start();
        let names_it = setting
            .strip_prefix(name)
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if names_it && !replaced {
            lines.push(assignment.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(assignment);
    }
    let mut contents = lines.join("\n");
    contents.push('\n');
    // Refuse to write anything the loader would then reject.
    for entry in dotenvy::from_read_iter(contents.as_bytes()) {
        entry.map_err(|_| format!("{}: the updated file would not parse", path.display()))?;
    }
    let directory = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("Cannot create {}: {e}", directory.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|e| format!("Cannot write beside {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Cannot restrict {}: {e}", path.display()))?;
    }
    temporary
        .write_all(contents.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|e| format!("Cannot replace {}: {}", path.display(), e.error))?;
    Ok(())
}

impl AppConfig {
    /// `load_with` without a persisted Windows environment.
    #[cfg(test)]
    pub fn load(
        base: &Path,
        portable: bool,
        environment: &HashMap<String, String>,
    ) -> Result<Self, String> {
        Self::load_with(base, portable, environment, &HashMap::new())
    }

    /// Reads `.env`, then lets the persisted Windows environment and finally the
    /// process environment override it. The API key is taken from the first of
    /// those three that has one, so a placeholder or blank value never hides a
    /// real key further down.
    pub fn load_with(
        base: &Path,
        portable: bool,
        environment: &HashMap<String, String>,
        windows: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let env_path = base.join(".env");
        if portable {
            create_missing(&env_path, PORTABLE_ENV.as_bytes())?;
            // Create both first-run templates even when .env is malformed or the key is unset.
            VoiceMappings::create_missing(&base.join("voices.json"))?;
        }
        let dotenv = read_dotenv(&env_path, portable)?;
        let mut values = dotenv.clone();
        values.extend(windows.clone());
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
        let key = [
            (environment, KeyOrigin::Process),
            (windows, KeyOrigin::Windows),
            (&dotenv, KeyOrigin::DotEnv),
        ]
        .into_iter()
        .find_map(|(source, origin)| {
            source
                .get("GEMINI_API_KEY")
                .filter(|key| !key.trim().is_empty() && key.trim() != PLACEHOLDER_KEY)
                .map(|key| (key.clone(), origin))
        });
        if key
            .as_ref()
            .is_some_and(|(key, _)| key.chars().any(|c| c.is_whitespace() || c.is_control()))
        {
            return Err(setting_error(
                "GEMINI_API_KEY",
                "must not contain whitespace or control characters",
            ));
        }
        let (gemini_api_key, key_origin) = key.unzip();
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
        let thinking_level = value("GEMINI_THINKING_LEVEL", DEFAULT_THINKING_LEVEL)
            .trim()
            .to_ascii_lowercase();
        if !matches!(thinking_level.as_str(), "low" | "medium" | "high") {
            return Err(setting_error(
                "GEMINI_THINKING_LEVEL",
                "must be low, medium or high",
            ));
        }
        let exam_budget_micro_usd = value("EXAM_BUDGET_USD", DEFAULT_EXAM_BUDGET_USD)
            .trim()
            .trim_start_matches('$')
            .parse::<f64>()
            .ok()
            .filter(|usd| usd.is_finite() && (0.0..=1_000.0).contains(usd))
            .map(|usd| (usd * 1_000_000.0).round() as u64)
            .ok_or_else(|| {
                setting_error(
                    "EXAM_BUDGET_USD",
                    "must be an amount in US dollars such as 0.70; 0 means no budget",
                )
            })?;
        let speech_cache_hours = value(
            "SPEECH_CACHE_HOURS",
            &DEFAULT_SPEECH_CACHE_HOURS.to_string(),
        )
        .trim()
        .parse::<u32>()
        .map_err(|_| {
            setting_error(
                "SPEECH_CACHE_HOURS",
                "must be a whole number of hours; 0 turns speech reuse off",
            )
        })?;
        let ip = value("IP", "127.0.0.1")
            .parse::<IpAddr>()
            .map_err(|_| setting_error("IP", "must be a valid IPv4 or IPv6 address"))?;
        let port = value("PORT", "8080")
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| setting_error("PORT", "must be an integer from 1 to 65535"))?;
        let audio_retention_hours = value(
            "AUDIO_RETENTION_HOURS",
            &DEFAULT_AUDIO_RETENTION_HOURS.to_string(),
        )
        .trim()
        .parse::<u32>()
        .map_err(|_| {
            setting_error(
                "AUDIO_RETENTION_HOURS",
                "must be a whole number of hours; 0 keeps recordings until their exam is deleted",
            )
        })?;
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
            gemini_api_key,
            key_origin,
            dotenv_path: env_path,
            text_model,
            tts_model,
            thinking_level,
            exam_budget_micro_usd,
            speech_cache_hours,
            voices,
            music_path,
            audio_retention_hours,
            address: SocketAddr::new(ip, port),
            log_filter,
        })
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.data_dir.join("audio")
    }
    /// Where reusable synthesised chunks live (`tts::cache`).
    pub fn speech_cache_dir(&self) -> PathBuf {
        self.audio_dir().join("cache")
    }
    /// The age after which a cached chunk is deleted; `None` when the cache is off.
    pub fn speech_cache_secs(&self) -> Option<i64> {
        (self.speech_cache_hours > 0).then(|| i64::from(self.speech_cache_hours) * 3_600)
    }
    /// The age after which an unreferenced recording is purged; `None` never purges.
    pub fn audio_retention_secs(&self) -> Option<i64> {
        (self.audio_retention_hours > 0).then(|| i64::from(self.audio_retention_hours) * 3_600)
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
    fn first_launch_creates_both_files_and_starts_without_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(dir.path(), true, &HashMap::new()).unwrap();
        assert!(cfg.gemini_api_key.is_none() && cfg.key_origin.is_none());
        assert!(dir.path().join(".env").is_file());
        assert!(dir.path().join("voices.json").is_file());
        assert_eq!(cfg.dotenv_path, dir.path().join(".env"));
    }

    #[test]
    fn key_comes_from_process_then_windows_then_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "GEMINI_API_KEY=from-dotenv\n").unwrap();
        let process = HashMap::from([("GEMINI_API_KEY".into(), "from-process".into())]);
        let windows = HashMap::from([("GEMINI_API_KEY".into(), "from-windows".into())]);
        let none = HashMap::new();
        let origin = |environment: &HashMap<String, String>, windows: &HashMap<String, String>| {
            let cfg = AppConfig::load_with(dir.path(), true, environment, windows).unwrap();
            (cfg.gemini_api_key.unwrap(), cfg.key_origin.unwrap())
        };
        assert_eq!(
            origin(&process, &windows),
            ("from-process".into(), KeyOrigin::Process)
        );
        assert_eq!(
            origin(&none, &windows),
            ("from-windows".into(), KeyOrigin::Windows)
        );
        assert_eq!(
            origin(&none, &none),
            ("from-dotenv".into(), KeyOrigin::DotEnv)
        );
        // A blank or placeholder value never hides a real key further down.
        let blank = HashMap::from([("GEMINI_API_KEY".into(), " ".into())]);
        let placeholder = HashMap::from([("GEMINI_API_KEY".into(), PLACEHOLDER_KEY.into())]);
        assert_eq!(
            origin(&blank, &placeholder),
            ("from-dotenv".into(), KeyOrigin::DotEnv)
        );
        std::fs::write(
            dir.path().join(".env"),
            "GEMINI_API_KEY=your_api_key_here\n",
        )
        .unwrap();
        let cfg = AppConfig::load_with(dir.path(), true, &blank, &none).unwrap();
        assert!(cfg.gemini_api_key.is_none());
    }

    #[test]
    fn windows_settings_sit_between_dotenv_and_process() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "PORT=1111\nGEMINI_TEXT_MODEL=a\n").unwrap();
        let windows = HashMap::from([
            ("PORT".into(), "2222".into()),
            ("GEMINI_TEXT_MODEL".into(), "b".into()),
        ]);
        let mut process = environment();
        process.insert("PORT".into(), "3333".into());
        let cfg = AppConfig::load_with(dir.path(), true, &process, &windows).unwrap();
        assert_eq!(cfg.address.port(), 3333);
        assert_eq!(cfg.text_model, "b");
    }

    #[test]
    fn remembering_a_key_keeps_every_other_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(
            &path,
            "# comment\nIP=127.0.0.1\nGEMINI_API_KEY=your_api_key_here\n# GEMINI_TEXT_MODEL=x\n",
        )
        .unwrap();
        remember_in_dotenv(&path, "GEMINI_API_KEY", "AIzaTestKey_0123456789-abc").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# comment\nIP=127.0.0.1\nGEMINI_API_KEY=AIzaTestKey_0123456789-abc\n# GEMINI_TEXT_MODEL=x\n"
        );
        let cfg = AppConfig::load(dir.path(), true, &HashMap::new()).unwrap();
        assert_eq!(cfg.key_origin, Some(KeyOrigin::DotEnv));

        std::fs::write(&path, "export GEMINI_API_KEY = old\nPORT=8080").unwrap();
        remember_in_dotenv(&path, "GEMINI_API_KEY", "new-key").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "GEMINI_API_KEY=new-key\nPORT=8080\n"
        );

        let fresh = dir.path().join("fresh/.env");
        remember_in_dotenv(&fresh, "GEMINI_API_KEY", "new-key").unwrap();
        assert_eq!(
            std::fs::read_to_string(&fresh).unwrap(),
            "GEMINI_API_KEY=new-key\n"
        );
    }

    #[test]
    fn browser_keys_must_look_like_keys() {
        assert!(browser_key_problem("AIzaSyA-valid_looking0123456789abcdef").is_none());
        for bad in [
            "",
            PLACEHOLDER_KEY,
            "short",
            "AIza key with spaces 0123456789",
            "AIzaSyA\"quote0123456789abcdef",
            "AIzaSyA#hash0123456789abcdef",
        ] {
            assert!(browser_key_problem(bad).is_some(), "{bad}");
        }
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
            ("AUDIO_RETENTION_HOURS", "abc"),
            ("AUDIO_RETENTION_HOURS", "-1"),
            ("GEMINI_THINKING_LEVEL", "minimal"),
            ("EXAM_BUDGET_USD", "cheap"),
            ("EXAM_BUDGET_USD", "-1"),
            ("SPEECH_CACHE_HOURS", "soon"),
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
    fn audio_retention_zero_means_never() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(dir.path(), true, &environment()).unwrap();
        assert_eq!(cfg.audio_retention_hours, DEFAULT_AUDIO_RETENTION_HOURS);
        assert_eq!(cfg.audio_retention_secs(), Some(24 * 3_600));
        let mut env = environment();
        env.insert("AUDIO_RETENTION_HOURS".into(), " 0 ".into());
        let cfg = AppConfig::load(dir.path(), true, &env).unwrap();
        assert_eq!(cfg.audio_retention_secs(), None);
        assert!(
            std::fs::read_to_string(dir.path().join(".env"))
                .unwrap()
                .contains("# AUDIO_RETENTION_HOURS=24")
        );
    }

    #[test]
    fn generation_settings_have_cost_minded_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(dir.path(), true, &environment()).unwrap();
        assert_eq!(cfg.text_model, "gemini-3.8-flash");
        assert_eq!(cfg.tts_model, "gemini-3.8-flash-tts");
        assert_eq!(cfg.thinking_level, "low");
        assert_eq!(cfg.exam_budget_micro_usd, 700_000);
        assert_eq!(cfg.speech_cache_secs(), Some(72 * 3_600));
        let mut env = environment();
        env.insert("GEMINI_THINKING_LEVEL".into(), " Medium ".into());
        env.insert("EXAM_BUDGET_USD".into(), "$1.25".into());
        env.insert("SPEECH_CACHE_HOURS".into(), "0".into());
        let cfg = AppConfig::load(dir.path(), true, &env).unwrap();
        assert_eq!(cfg.thinking_level, "medium");
        assert_eq!(cfg.exam_budget_micro_usd, 1_250_000);
        assert_eq!(cfg.speech_cache_secs(), None);
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
