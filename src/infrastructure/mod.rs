//! Server-only adapters and process startup.
#![cfg(feature = "server")]

pub mod audio;
pub mod config;
pub mod console;
pub mod exams;
pub mod ingress;
pub mod instance;
pub mod jobs;
pub mod listeners;
pub mod llm;
pub mod prompts;
pub mod rate_limiter;
pub mod secrets;
pub mod startup;
pub mod tts;
pub mod usage;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// The first half of startup: configuration, data folders and logging.
/// Nothing is shared with other processes yet (no lock, no port, no job
/// database), so a second copy can still give way to a running one.
pub fn prepare(
    options: &config::StartupOptions,
) -> Result<
    (
        config::AppConfig,
        tracing_appender::non_blocking::WorkerGuard,
    ),
    String,
> {
    let base = options.directory()?;
    let cfg = config::AppConfig::load_with(
        &base,
        options.portable,
        &config::environment()?,
        &config::windows_environment(),
    )?;
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
    println!("Configuration: {}", base.display());
    Ok((cfg, guard))
}

/// The second half of startup, once this process owns its data folder and
/// ports: asks for a missing key at an interactive console (checked with
/// Google through `runtime`, which the dev path does not have, so it never
/// asks), reports the key and the notices, then freezes the configuration.
pub fn finish(
    mut cfg: config::AppConfig,
    options: &config::StartupOptions,
    runtime: Option<&tokio::runtime::Runtime>,
) -> Result<(), String> {
    if cfg.gemini_api_key.is_none()
        && let Some(runtime) = runtime
        && options.interactive(false)
        && let Some(key) = ask_for_key(
            &cfg.dotenv_path,
            cfg.address.ip().is_loopback(),
            console::ask_secret,
            console::ask_yes_no,
            |key| runtime.block_on(llm::GeminiClient::check_key(key)),
            |line| println!("{line}"),
        )
    {
        cfg.gemini_api_key = Some(key);
        cfg.key_origin = Some(config::KeyOrigin::DotEnv);
    }
    let key_note = key_note(
        cfg.key_origin,
        options.service.is_some(),
        cfg.address.ip().is_loopback(),
        &cfg.dotenv_path,
    );
    // Notices were collected while loading, before logging existed.
    let notices = cfg.notices.clone();
    config::initialize(cfg)?;
    println!("Gemini API key (GEMINI_API_KEY): {key_note}");
    tracing::info!("Gemini API key {key_note}");
    for notice in &notices {
        println!("Note: {notice}");
        tracing::warn!("{notice}");
    }
    Ok(())
}

/// Where the key came from, or what to do without one, for the startup line
/// "Gemini API key (GEMINI_API_KEY): …". A missing key always starts with
/// "missing" (the smoke test reads it); a service never asks at the console,
/// so its note names the `.env` it reads.
fn key_note(
    origin: Option<config::KeyOrigin>,
    service: bool,
    loopback: bool,
    dotenv: &std::path::Path,
) -> String {
    match origin {
        Some(origin) => format!("from {}", origin.describe()),
        None if service => format!(
            "missing; set GEMINI_API_KEY in {} and restart the service",
            dotenv.display()
        ),
        None if loopback => "missing; set GEMINI_API_KEY or enter it in the browser".into(),
        None => "missing; set GEMINI_API_KEY and restart".into(),
    }
}

/// The console key prompt, at most this many keys.
const KEY_ATTEMPTS: usize = 3;

/// What the console key prompt says, by what happens without a key: on a
/// loopback bind a browser on this computer may still enter one; on a
/// network bind every request is remote, so only a restart helps.
struct KeyTexts {
    prompt: &'static str,
    /// Enter alone, or no console input.
    skipped: &'static str,
    /// Three keys that could not be used.
    gave_up: &'static str,
    /// After the reason `.env` could not be written.
    not_saved: &'static str,
}

const BROWSER_ASKS: KeyTexts = KeyTexts {
    prompt: "Paste your Gemini API key (shown as *), or press Enter to enter it in the browser instead: ",
    skipped: "No key entered; the browser will ask for it.",
    gave_up: "No usable key after three tries; the browser will ask for it.",
    not_saved: "The browser will ask for the key.",
};

const RESTART_NEEDED: KeyTexts = KeyTexts {
    prompt: "Paste your Gemini API key (shown as *), or press Enter to skip: ",
    skipped: "No key entered; set GEMINI_API_KEY and restart.",
    gave_up: "No usable key after three tries; set GEMINI_API_KEY and restart.",
    not_saved: "Set GEMINI_API_KEY and restart.",
};

/// Asks the person at the console for a Gemini API key, checks it with Google
/// and saves it in `.env` (`dotenv`); returns the saved key. Enter alone (or
/// no console input) goes on without one, and so do three keys that could
/// not be used, or a `.env` that cannot be written: on a `loopback` bind the
/// browser then asks, otherwise the person is told to set the key and
/// restart. A key Google could not check (busy, offline) is saved only if
/// the person says so.
///
/// `ask` reads a masked key, `confirm` a [y/N] answer, `check` asks Google
/// and `say` prints a line: the console and the network, injected for tests.
/// Nothing here prints or logs the key.
fn ask_for_key(
    dotenv: &std::path::Path,
    loopback: bool,
    mut ask: impl FnMut(&str) -> Option<String>,
    mut confirm: impl FnMut(&str) -> bool,
    mut check: impl FnMut(&str) -> Result<(), llm::LlmError>,
    mut say: impl FnMut(&str),
) -> Option<String> {
    let texts = if loopback {
        &BROWSER_ASKS
    } else {
        &RESTART_NEEDED
    };
    say(&format!(
        "No Gemini API key was found in the environment or in {}.",
        dotenv.display()
    ));
    for _ in 0..KEY_ATTEMPTS {
        let typed = ask(texts.prompt).unwrap_or_default();
        let key = typed.trim();
        if key.is_empty() {
            say(texts.skipped);
            return None;
        }
        if let Some(problem) = config::browser_key_problem(key) {
            say(problem);
            continue;
        }
        let checked = match check(key) {
            Ok(()) => true,
            Err(error @ llm::LlmError::KeyRejected) => {
                say(&error.to_string());
                continue;
            }
            Err(error) => {
                say(&format!("Could not check the key with Google. {error}"));
                if !confirm("Save it anyway? [y/N] ") {
                    continue;
                }
                false
            }
        };
        return match config::remember_in_dotenv(dotenv, "GEMINI_API_KEY", key) {
            Ok(()) => {
                if checked {
                    say(&format!(
                        "Key checked with Google and saved in {}.",
                        dotenv.display()
                    ));
                } else {
                    say(&format!(
                        "Key saved in {} without a check by Google.",
                        dotenv.display()
                    ));
                }
                tracing::info!(
                    checked,
                    "Gemini API key entered at the console and saved in .env"
                );
                Some(key.to_string())
            }
            Err(error) => {
                say(&error);
                say(texts.not_saved);
                None
            }
        };
    }
    say(texts.gave_up);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm::LlmError;
    use std::collections::VecDeque;

    const KEY: &str = "AIzaTestKey_0123456789-abc";

    /// What one run of the console key prompt did.
    struct Run {
        saved: Option<String>,
        lines: Vec<String>,
        prompts: usize,
        questions: usize,
        checks: usize,
    }

    /// Runs the prompt on a loopback bind with these typed keys (`None`: no
    /// console input), Google's answers and [y/N] answers, in order.
    fn prompt(
        dotenv: &std::path::Path,
        typed: &[Option<&str>],
        google: Vec<Result<(), LlmError>>,
        answers: &[bool],
    ) -> Run {
        prompt_on(true, dotenv, typed, google, answers)
    }

    /// `prompt`, on a loopback bind or not.
    fn prompt_on(
        loopback: bool,
        dotenv: &std::path::Path,
        typed: &[Option<&str>],
        google: Vec<Result<(), LlmError>>,
        answers: &[bool],
    ) -> Run {
        let texts = if loopback {
            &BROWSER_ASKS
        } else {
            &RESTART_NEEDED
        };
        let mut typed: VecDeque<Option<String>> =
            typed.iter().map(|t| t.map(str::to_string)).collect();
        let mut google = VecDeque::from(google);
        let mut answers: VecDeque<bool> = answers.iter().copied().collect();
        let (mut lines, mut prompts, mut questions, mut checks) = (Vec::new(), 0, 0, 0);
        let saved = ask_for_key(
            dotenv,
            loopback,
            |text| {
                assert_eq!(text, texts.prompt);
                prompts += 1;
                typed.pop_front().expect("asked once too often")
            },
            |question| {
                assert_eq!(question, "Save it anyway? [y/N] ");
                questions += 1;
                answers.pop_front().expect("asked once too often")
            },
            |_| {
                checks += 1;
                google.pop_front().expect("checked once too often")
            },
            |line| lines.push(line.to_string()),
        );
        assert!(
            !lines.iter().any(|line| line.contains(KEY)),
            "the key was printed: {lines:?}"
        );
        Run {
            saved,
            lines,
            prompts,
            questions,
            checks,
        }
    }

    #[test]
    fn enter_alone_leaves_the_key_to_the_browser() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        for typed in [Some(""), Some("   "), None] {
            let run = prompt(&dotenv, &[typed], vec![], &[]);
            assert_eq!(run.saved, None);
            assert_eq!((run.prompts, run.checks), (1, 0));
            assert_eq!(
                run.lines,
                [
                    format!(
                        "No Gemini API key was found in the environment or in {}.",
                        dotenv.display()
                    ),
                    "No key entered; the browser will ask for it.".to_string(),
                ]
            );
            assert!(!dotenv.exists());
        }
    }

    #[test]
    fn on_a_network_bind_the_prompt_never_promises_the_browser() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        assert_eq!(
            RESTART_NEEDED.prompt,
            "Paste your Gemini API key (shown as *), or press Enter to skip: "
        );
        let run = prompt_on(false, &dotenv, &[Some("")], vec![], &[]);
        assert_eq!(run.saved, None);
        assert_eq!(
            run.lines[1..],
            ["No key entered; set GEMINI_API_KEY and restart."]
        );
        let run = prompt_on(
            false,
            &dotenv,
            &[Some("short"), Some("short"), Some("short")],
            vec![],
            &[],
        );
        assert_eq!(
            run.lines.last().map(String::as_str),
            Some("No usable key after three tries; set GEMINI_API_KEY and restart.")
        );
        std::fs::create_dir(&dotenv).unwrap();
        let run = prompt_on(false, &dotenv, &[Some(KEY)], vec![Ok(())], &[]);
        assert_eq!(run.saved, None);
        assert_eq!(
            run.lines.last().map(String::as_str),
            Some("Set GEMINI_API_KEY and restart.")
        );
        for line in [
            RESTART_NEEDED.prompt,
            RESTART_NEEDED.skipped,
            RESTART_NEEDED.gave_up,
            RESTART_NEEDED.not_saved,
        ] {
            assert!(!line.contains("browser"), "{line}");
        }
    }

    #[test]
    fn a_key_google_accepts_is_saved_in_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        std::fs::write(&dotenv, "PORT=8080\nGEMINI_API_KEY=your_api_key_here\n").unwrap();
        // A malformed key is refused before Google is asked; spaces around a
        // pasted key do not count.
        let run = prompt(
            &dotenv,
            &[Some("not a key"), Some(&format!("  {KEY} "))],
            vec![Ok(())],
            &[],
        );
        assert_eq!(run.saved.as_deref(), Some(KEY));
        assert_eq!((run.prompts, run.checks, run.questions), (2, 1, 0));
        assert_eq!(
            run.lines[1..],
            [
                config::browser_key_problem("not a key")
                    .unwrap()
                    .to_string(),
                format!("Key checked with Google and saved in {}.", dotenv.display()),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&dotenv).unwrap(),
            format!("PORT=8080\nGEMINI_API_KEY={KEY}\n")
        );
    }

    #[test]
    fn three_rejected_keys_end_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        let run = prompt(
            &dotenv,
            &[Some(KEY), Some(KEY), Some(KEY)],
            vec![
                Err(LlmError::KeyRejected),
                Err(LlmError::KeyRejected),
                Err(LlmError::KeyRejected),
            ],
            &[],
        );
        assert_eq!(run.saved, None);
        assert_eq!((run.prompts, run.checks, run.questions), (3, 3, 0));
        let rejected = LlmError::KeyRejected.to_string();
        assert_eq!(
            run.lines[1..],
            [
                rejected.as_str(),
                &rejected,
                &rejected,
                "No usable key after three tries; the browser will ask for it.",
            ]
        );
        assert!(!dotenv.exists());
    }

    #[test]
    fn malformed_keys_use_up_the_tries_without_asking_google() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        let run = prompt(
            &dotenv,
            &[Some("short"), Some("your_api_key_here"), Some("not a key")],
            vec![],
            &[],
        );
        assert_eq!(run.saved, None);
        assert_eq!((run.prompts, run.checks, run.questions), (3, 0, 0));
        assert_eq!(
            run.lines.last().map(String::as_str),
            Some("No usable key after three tries; the browser will ask for it.")
        );
        assert!(!dotenv.exists());
    }

    #[test]
    fn a_rejected_key_can_be_followed_by_a_good_one() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        let run = prompt(
            &dotenv,
            &[Some("AIzaWrongKey_0123456789"), Some(KEY)],
            vec![Err(LlmError::KeyRejected), Ok(())],
            &[],
        );
        assert_eq!(run.saved.as_deref(), Some(KEY));
        assert_eq!((run.prompts, run.checks, run.questions), (2, 2, 0));
        assert_eq!(run.lines[1], LlmError::KeyRejected.to_string());
        assert_eq!(
            std::fs::read_to_string(&dotenv).unwrap(),
            format!("GEMINI_API_KEY={KEY}\n")
        );
    }

    #[test]
    fn a_missing_key_note_starts_with_missing() {
        let dotenv = std::path::Path::new("dir").join(".env");
        let note = |origin, service, loopback| key_note(origin, service, loopback, &dotenv);
        assert_eq!(
            note(Some(config::KeyOrigin::DotEnv), true, true),
            "from .env"
        );
        assert_eq!(
            note(None, true, true),
            format!(
                "missing; set GEMINI_API_KEY in {} and restart the service",
                dotenv.display()
            )
        );
        assert_eq!(
            note(None, false, true),
            "missing; set GEMINI_API_KEY or enter it in the browser"
        );
        assert_eq!(
            note(None, false, false),
            "missing; set GEMINI_API_KEY and restart"
        );
        for (service, loopback) in [(true, false), (false, true), (false, false)] {
            // The smoke test reads "GEMINI_API_KEY): missing".
            assert!(note(None, service, loopback).starts_with("missing"));
        }
    }

    #[test]
    fn an_unchecked_key_is_saved_only_when_the_person_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        let run = prompt(
            &dotenv,
            &[Some(KEY), Some(KEY)],
            vec![Err(LlmError::Timeout), Err(LlmError::Busy)],
            &[false, true],
        );
        assert_eq!(run.saved.as_deref(), Some(KEY));
        assert_eq!((run.prompts, run.checks, run.questions), (2, 2, 2));
        assert_eq!(
            run.lines[1..],
            [
                format!("Could not check the key with Google. {}", LlmError::Timeout),
                format!("Could not check the key with Google. {}", LlmError::Busy),
                format!(
                    "Key saved in {} without a check by Google.",
                    dotenv.display()
                ),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&dotenv).unwrap(),
            format!("GEMINI_API_KEY={KEY}\n")
        );
    }

    #[test]
    fn a_dotenv_that_cannot_be_written_leaves_the_key_to_the_browser() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where `.env` should be: it cannot be read as a file.
        let dotenv = dir.path().join(".env");
        std::fs::create_dir(&dotenv).unwrap();
        let run = prompt(&dotenv, &[Some(KEY)], vec![Ok(())], &[]);
        assert_eq!(run.saved, None);
        assert_eq!((run.prompts, run.checks), (1, 1));
        assert_eq!(run.lines.len(), 3, "{:?}", run.lines);
        assert!(run.lines[1].starts_with("Cannot read "), "{:?}", run.lines);
        assert_eq!(run.lines[2], "The browser will ask for the key.");
    }
}
