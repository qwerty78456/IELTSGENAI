//! Distribution wrapper only; application logic remains in the Dioxus crate.
use std::{
    io::{self, IsTerminal},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

/// Set once the extracted payload is gone (or was never created).
static CLEANED_UP: AtomicBool = AtomicBool::new(false);
/// Set when Windows starts closing the console; no server may start after that.
static CLOSING: AtomicBool = AtomicBool::new(false);

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let pause = owns_console()
        && io::stdin().is_terminal()
        && !args.iter().any(|a| a == "--non-interactive");
    // A handler, not the inherited IGNORE flag: the child still receives Ctrl+C.
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(control), 1);
    }
    let code = match run(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ERROR: {error}");
            1
        }
    };
    CLEANED_UP.store(true, Ordering::Release);
    if code != 0 && pause {
        eprintln!("Press Enter to close.");
        let _ = io::stdin().read_line(&mut String::new());
    }
    std::process::exit(code);
}
#[cfg(windows)]
unsafe extern "system" fn control(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    };
    match event {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => 1,
        // Closing the window (the usual way to stop a double-clicked app) ends this
        // process as soon as the handler returns, and after about 5 s regardless.
        // The server exits on the same event; wait for main to remove the payload.
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => {
            CLOSING.store(true, Ordering::Release);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
            while !CLEANED_UP.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            1
        }
        _ => 0,
    }
}
fn owns_console() -> bool {
    #[cfg(windows)]
    {
        let mut processes = [0; 2];
        unsafe {
            windows_sys::Win32::System::Console::GetConsoleProcessList(processes.as_mut_ptr(), 2)
                == 1
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}
fn run(args: Vec<std::ffi::OsString>) -> Result<i32, Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let directory = exe
        .parent()
        .ok_or("Cannot locate the application directory")?;
    let payload = tempfile::Builder::new()
        .prefix("listening-generator-")
        .tempdir()?;
    let bytes = include_bytes!(env!("IELTS_PORTABLE_PAYLOAD"));
    zip::ZipArchive::new(io::Cursor::new(bytes))?.extract(payload.path())?;
    if CLOSING.load(Ordering::Acquire) {
        // Closed while extracting: a server started now would outlive its console.
        return Ok(1);
    }
    let status = Command::new(payload.path().join("server.exe"))
        .arg("--portable")
        .arg("--config-dir")
        .arg(directory)
        .args(args)
        .env("DIOXUS_PUBLIC_PATH", payload.path().join("public"))
        .status()?;
    if let Err(error) = payload.close() {
        eprintln!("Warning: temporary files could not be removed: {error}");
    }
    Ok(status.code().unwrap_or(1))
}
