//! The process of a running copy, checked to be this program before anything
//! is done to it: any local program can answer `GET /instance` with a made-up
//! process id, and that id must never get another program stopped.
//!
//! Windows opens the process once and keeps the handle for waiting and
//! terminating, so the id cannot be reused meanwhile. Linux reads
//! `/proc/{pid}/exe` again before every step. Elsewhere no process is ever
//! confirmed, so nothing is stopped.

use std::{
    path::Path,
    time::{Duration, Instant},
};

/// How often a wait looks at the process again.
const POLL: Duration = Duration::from_millis(100);

/// Whether `other` is the same program as `own`: the same executable file
/// name (`server.exe` in a portable folder and in a service's folder alike).
/// Windows file names ignore case.
pub fn same_program(own: &Path, other: &Path) -> bool {
    match (own.file_name(), other.file_name()) {
        (Some(own), Some(other)) if cfg!(windows) => own
            .to_string_lossy()
            .eq_ignore_ascii_case(&other.to_string_lossy()),
        (Some(own), Some(other)) => own == other,
        _ => false,
    }
}

/// Not 0 or 1 (no process of ours), within the range every platform's
/// process calls accept, and not this process.
pub fn plausible_pid(pid: u32, own_pid: u32) -> bool {
    (2..=i32::MAX as u32).contains(&pid) && pid != own_pid
}

/// A running copy's process, confirmed to run this program.
pub struct Process {
    /// Unix ids are not held open: every step checks this id again.
    #[cfg(not(windows))]
    pid: u32,
    #[cfg(windows)]
    handle: super::win32::ProcessHandle,
}

impl Process {
    /// Opens `pid` when it is another live process running this program;
    /// `None` otherwise (no such process, another program, no right to it).
    pub fn open_ours(pid: u32) -> Option<Self> {
        if !plausible_pid(pid, std::process::id()) {
            return None;
        }
        let own = std::env::current_exe().ok()?;
        #[cfg(windows)]
        {
            let handle = super::win32::ProcessHandle::open(pid).ok()?;
            let image = handle.image_path().ok()?;
            (same_program(&own, &image) && !handle.wait(Duration::ZERO)).then_some(Self { handle })
        }
        #[cfg(target_os = "linux")]
        {
            linux_runs(pid, &own).then_some(Self { pid })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = own;
            None
        }
    }

    /// Whether the process has ended. On Linux a process whose executable can
    /// no longer be read as this program (a zombie, a reused id) has ended
    /// too, as far as this copy is concerned.
    pub fn has_exited(&self) -> bool {
        #[cfg(windows)]
        {
            self.handle.wait(Duration::ZERO)
        }
        #[cfg(target_os = "linux")]
        {
            match std::env::current_exe() {
                Ok(own) => !linux_runs(self.pid, &own),
                Err(_) => false,
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            true
        }
    }

    /// Waits at most `timeout` for the process to end; whether it did.
    pub async fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.has_exited() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Ends the process at once. Never called on one that already ended (the
    /// id could be someone else's by then on Linux).
    pub fn terminate(&self) -> std::io::Result<()> {
        if self.has_exited() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            self.handle.terminate()
        }
        #[cfg(unix)]
        {
            // `plausible_pid` kept it in 2..=i32::MAX: never 0 or -1, which
            // would signal a whole group or every process of the user.
            let pid = libc::pid_t::try_from(self.pid)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
            // SAFETY: plain system call on a checked process id.
            if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(any(windows, unix)))]
        {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }
    }
}

/// Whether `pid` is alive and its executable is this program.
#[cfg(target_os = "linux")]
fn linux_runs(pid: u32, own: &Path) -> bool {
    // A zombie or another user's process has no readable `exe` link.
    std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| same_program(own, &exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_match_by_file_name() {
        let own = Path::new("C:/Apps/Listening/server.exe");
        assert!(same_program(own, Path::new("D:/Temp/payload/server.exe")));
        assert_eq!(
            same_program(own, Path::new("D:/Temp/payload/SERVER.EXE")),
            cfg!(windows)
        );
        assert!(!same_program(own, Path::new("C:/Windows/explorer.exe")));
        assert!(!same_program(
            own,
            Path::new("C:/Apps/Listening/server.exe.old")
        ));
        assert!(!same_program(own, Path::new("")));
        assert!(!same_program(Path::new(""), Path::new("")));
        let linux = Path::new("/tmp/.mount_abc/usr/bin/server");
        assert!(same_program(
            linux,
            Path::new("/tmp/.mount_xyz/usr/bin/server")
        ));
        // A replaced binary reads as "server (deleted)": not ours any more.
        assert!(!same_program(linux, Path::new("/usr/bin/server (deleted)")));
    }

    #[test]
    fn process_ids_in_range() {
        assert!(plausible_pid(2, 1000));
        assert!(plausible_pid(i32::MAX as u32, 1000));
        assert!(!plausible_pid(1000, 1000));
        assert!(!plausible_pid(1, 1000));
        assert!(!plausible_pid(0, 1000));
        assert!(!plausible_pid(i32::MAX as u32 + 1, 1000));
        assert!(!plausible_pid(u32::MAX, 1000));
    }

    #[test]
    fn this_process_and_impossible_ids_are_never_opened() {
        assert!(Process::open_ours(std::process::id()).is_none());
        for pid in [0, 1, u32::MAX] {
            assert!(Process::open_ours(pid).is_none(), "{pid}");
        }
    }

    /// Runs as the child of `a_copy_of_this_program_is_found_waited_on_and_ended`
    /// (the same test binary, so the same executable name); returns at once
    /// otherwise.
    #[test]
    #[ignore = "child process of another test"]
    fn sleeping_copy() {
        if std::env::var_os("LISTENING_EXAM_SLEEPING_COPY").is_some() {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[tokio::test]
    async fn a_copy_of_this_program_is_found_waited_on_and_ended() {
        use std::process::{Command, Stdio};
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "infrastructure::instance::process::tests::sleeping_copy",
            ])
            .env("LISTENING_EXAM_SLEEPING_COPY", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let process = Process::open_ours(child.id()).expect("the copy is this program");
        assert!(!process.wait_exit(Duration::from_millis(300)).await);
        process.terminate().unwrap();
        // On Linux the child stays a zombie until reaped; reap it as its parent would.
        let reaped = tokio::task::spawn_blocking(move || child.wait());
        assert!(process.wait_exit(Duration::from_secs(5)).await);
        reaped.await.unwrap().unwrap();
        // Ending an ended process does nothing.
        process.terminate().unwrap();
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn another_program_is_not_ours() {
        use std::process::{Command, Stdio};
        let mut command = if cfg!(windows) {
            let mut ping = Command::new("ping");
            ping.args(["-n", "30", "127.0.0.1"]);
            ping
        } else {
            let mut sleep = Command::new("sleep");
            sleep.arg("30");
            sleep
        };
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let found = Process::open_ours(child.id()).is_some();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!found);
    }
}
