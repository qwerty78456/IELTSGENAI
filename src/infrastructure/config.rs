//! Explicit, fallible startup configuration. Never print dotenv contents or secrets.
use std::{
    collections::HashMap,
    io::{ErrorKind, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{audio::Pcm16, tts::voices::VoiceCatalog};

pub const DEFAULT_TEXT_MODEL: &str = "gemini-3.8-flash";
pub const DEFAULT_TTS_MODEL: &str = "gemini-3.8-flash-tts";
/// Writes the few-word summaries in download file names: the cheapest text model.
pub const DEFAULT_SUMMARY_MODEL: &str = "gemini-3.5-flash-lite";
pub const DEFAULT_THINKING_LEVEL: &str = "low";
pub const DEFAULT_EXAM_BUDGET_USD: &str = "0.70";
pub const DEFAULT_SPEECH_CACHE_HOURS: u32 = 72;
pub const TTS_MAX_INPUT_TOKENS: usize = 8_192;
pub const DEFAULT_AUDIO_RETENTION_HOURS: u32 = 24;
/// The value `.env.example` and the portable template ship with; it means "no key".
const PLACEHOLDER_KEY: &str = "your_api_key_here";
const PORTABLE_ENV: &str = "# Listening Exam Generator. Restart after editing.\nIP=127.0.0.1\nPORT=8080\n# PUBLIC_PORT=8081   # Cloudflare Tunnel port (same IP); requests on it count as internet users\n# PUBLIC_HOST=app.example.com   # required with PUBLIC_PORT: the tunnel's hostname(s), comma-separated\nDATA_DIR=./data\nVOICES_PATH=./voices.json\n# Leave the key out to use the GEMINI_API_KEY environment variable, or to be asked for it at the console or in the browser.\nGEMINI_API_KEY=your_api_key_here\n# GEMINI_TEXT_MODEL=gemini-3.8-flash\n# GEMINI_TTS_MODEL=gemini-3.8-flash-tts\n# GEMINI_SUMMARY_MODEL=gemini-3.5-flash-lite\n# GEMINI_THINKING_LEVEL=low\n# EXAM_BUDGET_USD=0.70\n# SPEECH_CACHE_HOURS=72\n# MUSIC_PATH=./music.wav\n# AUDIO_RETENTION_HOURS=24\nRUST_LOG=info\n";

/// Every setting the server reads, from the process environment, the persisted
/// Windows environment and `.env`.
const SETTINGS: [&str; 16] = [
    "GEMINI_API_KEY",
    "GEMINI_TEXT_MODEL",
    "GEMINI_TTS_MODEL",
    "GEMINI_SUMMARY_MODEL",
    "GEMINI_THINKING_LEVEL",
    "EXAM_BUDGET_USD",
    "SPEECH_CACHE_HOURS",
    "DATA_DIR",
    "VOICES_PATH",
    "MUSIC_PATH",
    "AUDIO_RETENTION_HOURS",
    "IP",
    "PORT",
    "PUBLIC_PORT",
    "PUBLIC_HOST",
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
    /// `--service NAME`: run by a service manager under that service name. Never
    /// asks anything and never opens a browser.
    pub service: Option<String>,
}

const SERVICE_NAME_ERROR: &str =
    "--service needs a service name made of letters, digits, '.', '_' or '-'";

/// A service name the app may print and later hand to the service manager:
/// 1 to 80 ASCII letters, digits, `.`, `_` or `-`, not starting with `-` (so
/// `--service --portable` is a missing name, not a service called "--portable").
pub fn valid_service_name(name: &str) -> bool {
    (1..=80).contains(&name.len())
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
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
                Some("--service") => {
                    let name = args.next().ok_or(SERVICE_NAME_ERROR)?;
                    match name.to_str() {
                        Some(name) if valid_service_name(name) => {
                            options.service = Some(name.to_owned());
                        }
                        _ => return Err(SERVICE_NAME_ERROR.into()),
                    }
                }
                _ => return Err("Unknown argument. Supported: --portable --config-dir PATH --no-open --non-interactive --service NAME".into()),
            }
        }
        Ok(options)
    }

    /// Whether a portable run opens the browser once the server listens.
    pub fn opens_browser(&self) -> bool {
        self.portable && !self.no_open && self.service.is_none()
    }

    /// Whether this run may ask the person at the console (a key, a [y/N]):
    /// never as a service, with `--non-interactive`, under `dx serve`, in
    /// Windows' session 0, or when stdin or stdout is not a real console.
    pub fn interactive(&self, dev_path: bool) -> bool {
        super::console::interactive(
            self.service.is_some(),
            self.non_interactive,
            dev_path,
            super::console::in_session_zero(),
            super::console::stdin_is_console(),
            super::console::stdout_is_console(),
        )
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
    /// `None` when no source has a key. An interactive console run then asks
    /// for one and saves it in `.env` (`infrastructure::finish`); otherwise a
    /// browser on the server's own computer may supply one
    /// (`infrastructure::secrets`).
    pub gemini_api_key: Option<String>,
    pub key_origin: Option<KeyOrigin>,
    /// The `.env` this configuration was read from (it may not exist).
    pub dotenv_path: PathBuf,
    pub text_model: String,
    pub tts_model: String,
    /// The model for download-name summaries. Its requests carry no thinking
    /// level: each model thinks at its default (minimal on Flash-Lite).
    pub summary_model: String,
    /// `generation_config.thinking_level` for text requests: low, medium or high.
    pub thinking_level: String,
    /// What one exam should cost at most, in µUSD; 0 means no budget. Only warned about.
    pub exam_budget_micro_usd: u64,
    /// How long a synthesised chunk is kept for reuse; 0 disables the speech cache.
    pub speech_cache_hours: u32,
    /// The voices speakers and the announcer are read with: built-in pools
    /// plus the overrides in `voices.json`.
    pub voices: VoiceCatalog,
    /// Things the operator should know that do not stop startup (an ignored
    /// 0.7 `voices.json`, a voice pool too small). Printed and logged once
    /// logging is up.
    pub notices: Vec<String>,
    pub music_path: Option<PathBuf>,
    /// How long a recording no saved exam refers to is kept; 0 keeps every recording.
    pub audio_retention_hours: u32,
    pub address: SocketAddr,
    /// `IP` and `PUBLIC_PORT`: the published port for Cloudflare Tunnel, whose
    /// requests count as internet users (`ingress::Origin::Published`).
    pub public_address: Option<SocketAddr>,
    /// `PUBLIC_HOST`, lowercase: the tunnel's hostnames. A request on the
    /// published port is `Published` only when its `Host` (and `Origin`, if
    /// any) names one of them. Empty without `PUBLIC_PORT`.
    pub public_hosts: Vec<String>,
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

/// Why a key typed in the browser or pasted at the console key prompt cannot
/// be used, if it cannot. Stricter than the startup check: Google API keys
/// are letters, digits, `-` and `_`, and anything else could break the `.env`
/// line it may be written to.
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
/// over it, so a crash never leaves half a file. Only its owner may read it:
/// Unix permissions are 0600, and on Windows a protected DACL grants the
/// account running this process, SYSTEM and Administrators alone
/// (`owner_only`), both set before the key is written. `value` must already be safe unquoted (see
/// `browser_key_problem`).
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
    #[cfg(windows)]
    owner_only(&temporary).map_err(|e| format!("Cannot restrict {}: {e}", path.display()))?;
    temporary
        .write_all(contents.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|e| format!("Cannot replace {}: {}", path.display(), e.error))?;
    Ok(())
}

/// The Windows counterpart of 0600: a protected DACL (no inherited entries)
/// granting full control to the account running this process (by its SID:
/// an elevated run's files are owned by Administrators, so "owner" would
/// lock that account out once it runs without elevation), SYSTEM and
/// Administrators only, so other accounts on the computer cannot read the
/// key. A volume without permissions (FAT or exFAT, a USB stick) cannot keep
/// one: the file is then written as everything else on it is, and the
/// console and the log say so.
#[cfg(windows)]
fn owner_only(file: &tempfile::NamedTempFile) -> std::io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            SetFileSecurityW,
        },
        Storage::FileSystem::GetVolumeInformationByHandleW,
    };
    /// The volume keeps permissions (`FILE_PERSISTENT_ACLS`, winnt.h).
    const FILE_PERSISTENT_ACLS: u32 = 0x8;

    let mut flags = 0u32;
    // SAFETY: the handle is the open temporary file; only `flags` is written,
    // every other output is null with a zero length.
    let known = unsafe {
        GetVolumeInformationByHandleW(
            file.as_file().as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            std::ptr::null_mut(),
            0,
        )
    } != 0;
    if known && flags & FILE_PERSISTENT_ACLS == 0 {
        let folder = file.path().parent().unwrap_or(file.path()).display();
        let warning = format!(
            "{folder} is on a drive without file permissions (FAT or exFAT), so other accounts on this computer can read the saved key."
        );
        println!("Note: {warning}");
        tracing::warn!("{warning}");
        return Ok(());
    }
    let wide = |text: &std::ffi::OsStr| -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    };
    let sddl = wide(owner_only_sddl(&current_user_sid()?).as_ref());
    let path = wide(file.path().as_os_str());
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the calls;
    // the descriptor Windows allocates is freed with LocalFree below.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let set = SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        let error = std::io::Error::last_os_error();
        LocalFree(descriptor);
        if set == 0 {
            return Err(error);
        }
    }
    Ok(())
}

/// `PUBLIC_HOST`: one or more hostnames separated by commas (spaces around
/// them and empty entries ignored), each made of non-empty labels of ASCII
/// letters, digits and `-`, joined by `.`, at most 253 characters; returned
/// lowercase. `None` when any entry is not such a name (a port, a scheme, a
/// path) or there is none.
fn parse_public_hosts(text: &str) -> Option<Vec<String>> {
    let mut hosts = Vec::new();
    for host in text
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
    {
        let valid = host.len() <= 253
            && host.split('.').all(|label| {
                (1..=63).contains(&label.len())
                    && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            });
        if !valid {
            return None;
        }
        let host = host.to_ascii_lowercase();
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    (!hosts.is_empty()).then_some(hosts)
}

/// The protected DACL `owner_only` sets: full access for `user` (a SID
/// string), SYSTEM and built-in Administrators, nothing inherited.
#[cfg(windows)]
fn owner_only_sddl(user: &str) -> String {
    format!("D:P(A;;FA;;;{user})(A;;FA;;;SY)(A;;FA;;;BA)")
}

/// The SID of the account this process runs as (its token's user, elevated
/// or not), as a string such as `S-1-5-21-…-1001`.
#[cfg(windows)]
fn current_user_sid() -> std::io::Result<String> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no
    // closing; `token` receives a real handle, owned from here on.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a valid token handle OpenProcessToken returned.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let raw = {
        use std::os::windows::io::AsRawHandle;
        token.as_raw_handle()
    };
    let mut needed = 0u32;
    // SAFETY: the first call only reports the size the information needs.
    unsafe { GetTokenInformation(raw, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // u64 keeps the TOKEN_USER (and the SID after it) aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer holds `needed` bytes, the size the call asked for.
    if unsafe {
        GetTokenInformation(
            raw,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the call filled the buffer with a TOKEN_USER whose SID points
    // into the same buffer, alive until the end of this function.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` is valid (above); Windows allocates the string, freed
    // with LocalFree once copied.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let length = (0..).take_while(|&i| *text.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        Ok(sid)
    }
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
            VoiceCatalog::create_missing(&base.join("voices.json"))?;
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
        VoiceCatalog::create_missing(&voices_path)?;
        let (voices, mut notices) = VoiceCatalog::load(&voices_path)?;
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
        let summary_model = value("GEMINI_SUMMARY_MODEL", DEFAULT_SUMMARY_MODEL);
        for (name, model) in [
            ("GEMINI_TEXT_MODEL", &text_model),
            ("GEMINI_TTS_MODEL", &tts_model),
            ("GEMINI_SUMMARY_MODEL", &summary_model),
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
        let public_port = match values.get("PUBLIC_PORT").map(|p| p.trim()) {
            None | Some("") => None,
            Some(text) => Some(
                text.parse::<u16>()
                    .ok()
                    .filter(|p| *p > 0 && *p != port)
                    .ok_or_else(|| {
                        setting_error(
                            "PUBLIC_PORT",
                            "must be an integer from 1 to 65535, different from PORT; leave it empty for no tunnel port",
                        )
                    })?,
            ),
        };
        let public_host = values
            .get("PUBLIC_HOST")
            .map(|hosts| hosts.trim())
            .filter(|hosts| !hosts.is_empty());
        let public_hosts = match (public_port, public_host) {
            (Some(_), Some(hosts)) => parse_public_hosts(hosts).ok_or_else(|| {
                setting_error(
                    "PUBLIC_HOST",
                    "must be hostnames made of letters, digits, '-' and '.', separated by commas, without a port or scheme, for example app.example.com",
                )
            })?,
            (Some(_), None) => {
                return Err(setting_error(
                    "PUBLIC_HOST",
                    "must name the Cloudflare Tunnel hostname, for example app.example.com, when PUBLIC_PORT is set",
                ));
            }
            (None, Some(_)) => {
                notices.push("PUBLIC_HOST is ignored without PUBLIC_PORT.".into());
                Vec::new()
            }
            (None, None) => Vec::new(),
        };
        if let Some(public_port) = public_port
            && !ip.is_loopback()
        {
            notices.push(format!(
                "PUBLIC_PORT {public_port} is reachable from the network because IP={ip}, so it is treated like any other network address (no voice design). Use IP=127.0.0.1 with Cloudflare Tunnel."
            ));
        }
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
            summary_model,
            thinking_level,
            exam_budget_micro_usd,
            speech_cache_hours,
            voices,
            notices,
            music_path,
            audio_retention_hours,
            address: SocketAddr::new(ip, port),
            public_address: public_port.map(|port| SocketAddr::new(ip, port)),
            public_hosts,
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
    /// Where voice samples ("Preview") live, one WAV per voice id (`tts::samples`).
    pub fn voice_sample_dir(&self) -> PathBuf {
        self.audio_dir().join("voices")
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

    /// The DACL `remember_in_dotenv` leaves on `.env`, as SDDL.
    #[cfg(windows)]
    fn dacl_of(path: &Path) -> String {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
                },
                DACL_SECURITY_INFORMATION, GetFileSecurityW,
            },
        };
        let path: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: the first call only reports the size; the buffer then has
        // it; the SDDL string Windows allocates is copied, then freed.
        unsafe {
            let mut needed = 0u32;
            GetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
            assert!(needed > 0, "{}", std::io::Error::last_os_error());
            // u64 keeps the descriptor aligned.
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
            assert_ne!(
                GetFileSecurityW(
                    path.as_ptr(),
                    DACL_SECURITY_INFORMATION,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                ),
                0,
                "{}",
                std::io::Error::last_os_error()
            );
            let mut text: *mut u16 = std::ptr::null_mut();
            let mut length = 0u32;
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    buffer.as_mut_ptr().cast(),
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    &mut length,
                ),
                0
            );
            let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, length as usize));
            LocalFree(text.cast());
            sddl.trim_end_matches('\0').to_string()
        }
    }

    #[cfg(windows)]
    #[test]
    fn remembered_keys_are_readable_by_their_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        // A fresh file, and one that replaces an existing (inheriting) file.
        remember_in_dotenv(&path, "GEMINI_API_KEY", "AIzaTestKey_0123456789-abc").unwrap();
        for round in 0..2 {
            let sddl = dacl_of(&path);
            // Protected: nothing inherited from the folder (which lets Users in).
            assert!(sddl.starts_with("D:P"), "{sddl}");
            // This account by its SID (SDDL writes the built-in Administrator
            // account, RID 500, as LA), never "whoever owns the file".
            let user = match current_user_sid().unwrap() {
                sid if sid.ends_with("-500") => "LA".to_string(),
                sid => sid,
            };
            assert!(user.starts_with("S-1-") || user == "LA", "{user}");
            for ace in [
                format!("(A;;FA;;;{user})"),
                "(A;;FA;;;SY)".into(),
                "(A;;FA;;;BA)".into(),
            ] {
                assert!(sddl.contains(&ace), "{ace} missing from {sddl}");
            }
            // Owner rights, Users, Authenticated Users, Everyone, Interactive.
            for anyone in [";OW)", ";BU)", ";AU)", ";WD)", ";IU)"] {
                assert!(!sddl.contains(anyone), "{anyone} in {sddl}");
            }
            assert_eq!(sddl.matches('(').count(), 3, "{sddl}");
            if round == 0 {
                std::fs::write(&path, "PORT=8080\n").unwrap();
                remember_in_dotenv(&path, "GEMINI_API_KEY", "new-key").unwrap();
            }
        }
        // The owner still reads and edits it.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "PORT=8080\nGEMINI_API_KEY=new-key\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_dacl_names_the_account_not_the_owner() {
        assert_eq!(
            owner_only_sddl("S-1-5-21-1-2-3-1001"),
            "D:P(A;;FA;;;S-1-5-21-1-2-3-1001)(A;;FA;;;SY)(A;;FA;;;BA)"
        );
        assert!(!owner_only_sddl("S-1-5-21-1-2-3-1001").contains("OW"));
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
            ("GEMINI_SUMMARY_MODEL", "bad/model"),
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
    fn first_run_writes_the_v2_template() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load(dir.path(), true, &environment()).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("voices.json")).unwrap())
                .unwrap();
        assert_eq!(written["version"], 2);
        assert_eq!(written["pools"], serde_json::json!({}));
        assert!(cfg.notices.is_empty(), "{:?}", cfg.notices);
        assert!(!cfg.voices.voices().is_empty());
        assert_eq!(cfg.voice_sample_dir(), cfg.audio_dir().join("voices"));
    }

    #[test]
    fn notices_report_a_0_7_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        let custom = r#"{"male": {"default": "Puck", "british": "Orus"}, "female": {"default": "Kore"}, "announcer": "Charon"}"#;
        std::fs::write(&path, custom).unwrap();
        let cfg = AppConfig::load(dir.path(), true, &environment()).unwrap();
        assert_eq!(cfg.notices.len(), 1);
        assert!(cfg.notices[0].contains("0.7 format"), "{:?}", cfg.notices);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), custom);
        assert_eq!(
            cfg.voices.voices(),
            crate::infrastructure::tts::voices::VoiceCatalog::builtin().voices()
        );
    }

    #[test]
    fn public_port_is_optional_and_must_differ_from_port() {
        let dir = tempfile::tempdir().unwrap();
        let load = |pairs: &[(&str, &str)]| {
            let mut env = environment();
            for (name, value) in pairs {
                env.insert((*name).into(), (*value).into());
            }
            AppConfig::load(dir.path(), true, &env)
        };
        let host = ("PUBLIC_HOST", "exams.example.org");
        // The template only mentions it in a comment.
        assert_eq!(load(&[]).unwrap().public_address, None);
        assert_eq!(load(&[("PUBLIC_PORT", " ")]).unwrap().public_address, None);
        let cfg = load(&[("PUBLIC_PORT", " 8081 "), host]).unwrap();
        assert_eq!(cfg.public_address, Some("127.0.0.1:8081".parse().unwrap()));
        assert!(!cfg.notices.iter().any(|n| n.contains("PUBLIC_PORT")));
        let cfg = load(&[("IP", "::1"), ("PUBLIC_PORT", "8081"), host]).unwrap();
        assert_eq!(cfg.public_address, Some("[::1]:8081".parse().unwrap()));
        for value in ["0", "8080", "65536", "-1", "abc", "80 81"] {
            let error = load(&[("PUBLIC_PORT", value), host]).err().unwrap();
            assert!(
                error.contains(
                    "PUBLIC_PORT must be an integer from 1 to 65535, different from PORT; leave it empty for no tunnel port"
                ),
                "{value}: {error}"
            );
        }
        let error = load(&[("PORT", "9000"), ("PUBLIC_PORT", "9000"), host])
            .err()
            .unwrap();
        assert!(error.contains("PUBLIC_PORT"), "{error}");
        assert!(load(&[("PORT", "9000"), ("PUBLIC_PORT", "8080"), host]).is_ok());
        // On a network address the published port is no tunnel-only door.
        let cfg = load(&[("IP", "0.0.0.0"), ("PUBLIC_PORT", "8081"), host]).unwrap();
        assert_eq!(cfg.public_address, Some("0.0.0.0:8081".parse().unwrap()));
        assert!(cfg.notices.iter().any(|n| n
            == "PUBLIC_PORT 8081 is reachable from the network because IP=0.0.0.0, so it is treated like any other network address (no voice design). Use IP=127.0.0.1 with Cloudflare Tunnel."));
        assert!(
            std::fs::read_to_string(dir.path().join(".env"))
                .unwrap()
                .contains("# PUBLIC_PORT=8081")
        );
    }

    #[test]
    fn public_host_names_the_tunnel_and_goes_with_public_port() {
        let dir = tempfile::tempdir().unwrap();
        let load = |pairs: &[(&str, &str)]| {
            let mut env = environment();
            for (name, value) in pairs {
                env.insert((*name).into(), (*value).into());
            }
            AppConfig::load(dir.path(), true, &env)
        };
        let port = ("PUBLIC_PORT", "8081");
        // Required with PUBLIC_PORT; blank counts as missing.
        for missing in [&[port][..], &[port, ("PUBLIC_HOST", "  ")]] {
            let error = load(missing).err().unwrap();
            assert!(
                error.ends_with(
                    "PUBLIC_HOST must name the Cloudflare Tunnel hostname, for example app.example.com, when PUBLIC_PORT is set"
                ),
                "{error}"
            );
        }
        let cfg = load(&[port, ("PUBLIC_HOST", "Exams.Example.org")]).unwrap();
        assert_eq!(cfg.public_hosts, ["exams.example.org"]);
        let cfg = load(&[
            port,
            (
                "PUBLIC_HOST",
                " a.example.com, B-2.example.net ,,a.example.com ",
            ),
        ])
        .unwrap();
        assert_eq!(cfg.public_hosts, ["a.example.com", "b-2.example.net"]);
        for bad in [
            "app.example.com:443",
            "https://app.example.com",
            "app.example.com/path",
            "app..example.com",
            ".example.com",
            "app_1.example.com",
            "app example.com",
            "*.example.com",
            "[::1]",
            "ứng-dụng.vn",
            ",",
        ] {
            let error = load(&[port, ("PUBLIC_HOST", bad)]).err().unwrap();
            assert!(
                error.contains("PUBLIC_HOST must be hostnames made of letters, digits"),
                "{bad}: {error}"
            );
        }
        let long = format!("{}.com", "a".repeat(64));
        assert!(load(&[port, ("PUBLIC_HOST", &long)]).is_err());
        // Without PUBLIC_PORT it is ignored, with a notice, whatever it says.
        for value in ["app.example.com", "not a host:1"] {
            let cfg = load(&[("PUBLIC_HOST", value)]).unwrap();
            assert!(cfg.public_hosts.is_empty());
            assert!(
                cfg.notices
                    .iter()
                    .any(|n| n == "PUBLIC_HOST is ignored without PUBLIC_PORT."),
                "{:?}",
                cfg.notices
            );
        }
        let cfg = load(&[("PUBLIC_HOST", "")]).unwrap();
        assert!(cfg.notices.is_empty(), "{:?}", cfg.notices);
        assert!(
            std::fs::read_to_string(dir.path().join(".env"))
                .unwrap()
                .contains("# PUBLIC_HOST=app.example.com")
        );
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
        assert!(options.service.is_none());
        let unknown = StartupOptions::parse(["--verbose".into()]).unwrap_err();
        assert_eq!(
            unknown,
            "Unknown argument. Supported: --portable --config-dir PATH --no-open --non-interactive --service NAME"
        );
    }

    #[test]
    fn service_names_are_checked() {
        let options =
            StartupOptions::parse(["--portable".into(), "--service".into(), "VMQ-MVP".into()])
                .unwrap();
        assert_eq!(options.service.as_deref(), Some("VMQ-MVP"));
        // A service never opens the browser, even when portable.
        assert!(!options.opens_browser());
        assert!(!options.interactive(false));
        let portable = StartupOptions::parse(["--portable".into()]).unwrap();
        assert!(portable.opens_browser());
        for name in ["a", "listening.exam_generator-2", &"x".repeat(80)] {
            assert!(valid_service_name(name), "{name}");
        }
        for name in [
            "",
            &"x".repeat(81),
            "two words",
            "quote'd",
            "semi;colon",
            "dollar$",
            "back`tick",
            "slash/",
            "dịch-vụ",
            "-x",
            "--portable",
        ] {
            assert!(!valid_service_name(name), "{name}");
        }
        let error = |args: &[&str]| {
            StartupOptions::parse(args.iter().map(|a| std::ffi::OsString::from(*a))).unwrap_err()
        };
        let expected = "--service needs a service name made of letters, digits, '.', '_' or '-'";
        assert_eq!(error(&["--service"]), expected);
        assert_eq!(error(&["--service", "a b"]), expected);
        assert_eq!(error(&["--service", ""]), expected);
        // The next option is not a name.
        assert_eq!(error(&["--service", "--portable"]), expected);
        assert_eq!(error(&["--portable", "--service", "--no-open"]), expected);
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
        assert_eq!(cfg.summary_model, "gemini-3.5-flash-lite");
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
