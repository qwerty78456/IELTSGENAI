//! The console a manual run was started from: whether someone can answer a
//! question there, and asking it (a [y/N] answer, a masked key).
//!
//! A service has no one at its console, and under NSSM stdin can still be a
//! (hidden) console, so `--service` and `--non-interactive` always win. On
//! Windows a process in session 0 (services, and Task Scheduler's "run
//! whether user is logged on or not") never asks either: its console, if it
//! has one, is on a desktop no one sees. Then both stdin and stdout must be a
//! real console.

/// Whether a run may ask at the console. Pure, for tests: the flags are the
/// command line, the path (`dx serve` never asks), whether the process runs
/// in Windows' session 0 (`in_session_zero`) and the console checks.
pub fn interactive(
    service: bool,
    non_interactive: bool,
    dev_path: bool,
    session_zero: bool,
    stdin_console: bool,
    stdout_console: bool,
) -> bool {
    !service && !non_interactive && !dev_path && !session_zero && stdin_console && stdout_console
}

/// Whether this process runs in Windows' session 0, where no one sits at a
/// console. Always `false` elsewhere, and when the session cannot be read.
pub fn in_session_zero() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::{
            RemoteDesktop::ProcessIdToSessionId, Threading::GetCurrentProcessId,
        };
        let mut session = u32::MAX;
        // SAFETY: plain calls; only `session` is written.
        let read = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } != 0;
        read && session == 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Asks a yes/no question on the console and reads one visible line. Only
/// "y" or "yes" (any case) mean yes; Enter alone, anything else, end of input
/// or a read error mean no. Call only when `interactive` said so.
pub fn ask_yes_no(question: &str) -> bool {
    use std::io::Write;
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => {
            // End of input: the answer line never came, so end the prompt's line.
            println!();
            false
        }
        Ok(_) => is_yes(&line),
    }
}

/// Asks for a secret on the console, showing `*` for each character typed or
/// pasted. `None` when nothing could be read (end of input, no console).
/// Call only when `interactive` said so: the read goes to the console itself
/// (`CONIN$`, `/dev/tty`), never to stdin, so it would block anywhere else.
///
/// Ctrl+C ends the program, with the console put back as it was: on Unix
/// rpassword then restores the terminal and this exits with code 130; on
/// Windows the system's own Ctrl+C exit runs after `interrupt::Guard` has
/// restored the input mode (the portable launcher's "Press Enter to close."
/// needs it).
pub fn ask_secret(prompt: &str) -> Option<String> {
    let config = rpassword::ConfigBuilder::new()
        .password_feedback_mask('*')
        .build();
    let read = {
        let _guard = interrupt::Guard::install();
        rpassword::prompt_password_with_config(prompt, config)
    };
    match read {
        Ok(secret) => Some(secret),
        // On Windows the read can fail (aborted) while the system's exit is
        // still on its way: end here too rather than carry on with no key.
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted || interrupt::happened() => {
            println!();
            std::process::exit(130);
        }
        Err(_) => None,
    }
}

/// Keeps Ctrl+C from leaving the console unusable while `ask_secret` reads.
#[cfg(unix)]
mod interrupt {
    /// SIGINT is ignored while it lives, so rpassword's own `raise(SIGINT)`
    /// does not kill the process before it restores the terminal; it returns
    /// `Interrupted` instead. The previous disposition comes back on drop.
    pub struct Guard {
        previous: libc::sighandler_t,
    }

    impl Guard {
        pub fn install() -> Self {
            // SAFETY: SIG_IGN is a valid disposition; nothing else in the
            // process changes SIGINT before the runtime serves.
            let previous = unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
            Self { previous }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let previous = match self.previous {
                libc::SIG_ERR => libc::SIG_DFL,
                previous => previous,
            };
            // SAFETY: restores the disposition `signal` returned above.
            unsafe { libc::signal(libc::SIGINT, previous) };
        }
    }

    /// rpassword reports Ctrl+C itself (`Interrupted`).
    pub fn happened() -> bool {
        false
    }
}

/// Keeps Ctrl+C from leaving the console unusable while `ask_secret` reads.
#[cfg(windows)]
mod interrupt {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use windows_sys::{
        Win32::{
            Foundation::FALSE,
            System::Console::{
                CONSOLE_MODE, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
                SetConsoleCtrlHandler, SetConsoleMode,
            },
        },
        core::BOOL,
    };

    /// The input mode from before the prompt.
    static MODE: AtomicU32 = AtomicU32::new(0);
    /// Set by the handler: Ctrl+C or Ctrl+Break reached the prompt.
    static HAPPENED: AtomicBool = AtomicBool::new(false);

    /// rpassword reads with `ENABLE_PROCESSED_INPUT` only, so Ctrl+C reaches
    /// the control handlers and the default one ends the process without
    /// restoring the console mode (no echo, no line input). While a `Guard`
    /// lives, a handler first puts the saved mode back, then lets the default
    /// exit continue.
    pub struct Guard {
        installed: bool,
    }

    impl Guard {
        pub fn install() -> Self {
            // SAFETY: GetStdHandle returns a borrowed handle; GetConsoleMode
            // writes only the u32 it is given; `restore` matches
            // PHANDLER_ROUTINE and stays valid for the whole process.
            let installed = unsafe {
                let mut mode: CONSOLE_MODE = 0;
                GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &mut mode) != 0 && {
                    MODE.store(mode, Ordering::SeqCst);
                    SetConsoleCtrlHandler(Some(restore), 1) != 0
                }
            };
            Self { installed }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.installed {
                // SAFETY: removes the routine added in `install`.
                unsafe { SetConsoleCtrlHandler(Some(restore), 0) };
            }
        }
    }

    /// Whether the handler ran during the prompt.
    pub fn happened() -> bool {
        HAPPENED.load(Ordering::SeqCst)
    }

    /// Runs on the system's control thread: restore, then FALSE, so the next
    /// handler (the default exit) still runs.
    unsafe extern "system" fn restore(_event: u32) -> BOOL {
        HAPPENED.store(true, Ordering::SeqCst);
        // SAFETY: the same borrowed console input handle as in `install`.
        unsafe { SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), MODE.load(Ordering::SeqCst)) };
        FALSE
    }
}

/// Other platforms: nothing to restore.
#[cfg(not(any(unix, windows)))]
mod interrupt {
    pub struct Guard;

    impl Guard {
        pub fn install() -> Self {
            Self
        }
    }

    pub fn happened() -> bool {
        false
    }
}

/// Whether a typed answer means yes.
pub fn is_yes(answer: &str) -> bool {
    let answer = answer.trim();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

/// Whether stdin is a real console (a terminal on Unix).
pub fn stdin_is_console() -> bool {
    #[cfg(windows)]
    {
        windows_console(windows_sys::Win32::System::Console::STD_INPUT_HANDLE)
    }
    #[cfg(not(windows))]
    {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    }
}

/// Whether stdout is a real console (a terminal on Unix).
pub fn stdout_is_console() -> bool {
    #[cfg(windows)]
    {
        windows_console(windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE)
    }
    #[cfg(not(windows))]
    {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal()
    }
}

/// `GetConsoleMode` succeeds only on a console handle. Unlike `IsTerminal`, a
/// msys/mintty pipe does not count: nothing could read a masked key there.
#[cfg(windows)]
fn windows_console(which: windows_sys::Win32::System::Console::STD_HANDLE) -> bool {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{GetConsoleMode, GetStdHandle},
    };
    // SAFETY: GetStdHandle takes a constant and returns a borrowed handle (or
    // null/invalid); GetConsoleMode only writes the u32 it is given.
    unsafe {
        let handle = GetStdHandle(which);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut mode = 0;
        GetConsoleMode(handle, &mut mode) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_manual_run_at_a_console_asks() {
        // service, non_interactive, dev_path, session_zero, stdin, stdout
        assert!(interactive(false, false, false, false, true, true));
        for flags in 0..64u8 {
            let bit = |n: u8| flags & (1 << n) != 0;
            let (service, non_interactive, dev, session_zero, stdin, stdout) =
                (bit(0), bit(1), bit(2), bit(3), bit(4), bit(5));
            // The one row that asks: no service, no flag, not dev, not
            // session 0, two consoles.
            let expected = flags == 0b110000;
            assert_eq!(
                interactive(service, non_interactive, dev, session_zero, stdin, stdout),
                expected,
                "service={service} non_interactive={non_interactive} dev={dev} session_zero={session_zero} stdin={stdin} stdout={stdout}"
            );
        }
        // NSSM: stdin may be a hidden console while stdout is a log file.
        assert!(!interactive(false, false, false, false, true, false));
        assert!(!interactive(true, false, false, false, true, true));
        // A scheduled task run "whether user is logged on or not": a console
        // on both, in session 0, with no one to answer.
        assert!(!interactive(false, false, false, true, true, true));
    }

    #[test]
    fn only_y_or_yes_means_yes() {
        for answer in [
            "y",
            "Y",
            "yes",
            "YES",
            "Yes",
            " y ",
            "yes\r\n",
            "y\n",
            "\tyEs\r\n",
        ] {
            assert!(is_yes(answer), "{answer:?}");
        }
        for answer in [
            "",
            "\n",
            "\r\n",
            "n",
            "N",
            "no",
            "ye",
            "yess",
            "yes please",
            "y y",
            "oui",
            "có",
            "1",
            "true",
        ] {
            assert!(!is_yes(answer), "{answer:?}");
        }
    }

    #[test]
    fn console_checks_do_not_panic() {
        // Under `cargo test` stdout is captured; the answer itself varies.
        let _ = stdin_is_console();
        let _ = stdout_is_console();
        // Tests run from a signed-in session, or from a service in session 0.
        let _ = in_session_zero();
    }
}
