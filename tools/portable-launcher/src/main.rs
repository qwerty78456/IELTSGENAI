//! Distribution wrapper only; application logic remains in the Dioxus crate.
use std::{
    io::{self, IsTerminal},
    process::Command,
};

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
    if code != 0 && pause {
        eprintln!("Press Enter to close.");
        let _ = io::stdin().read_line(&mut String::new());
    }
    std::process::exit(code);
}
#[cfg(windows)]
unsafe extern "system" fn control(event: u32) -> i32 {
    i32::from(event == 0 || event == 1)
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
