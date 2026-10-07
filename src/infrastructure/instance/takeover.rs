//! Taking over from a copy of the app that already runs here, when a person
//! at the console agrees (`[y/N]`, default no).
//!
//! A console copy is asked to stop (`POST /instance/stop` with the token from
//! its `instance.json`), waited for, and ended if it does not go. A Windows
//! service copy is stopped through the service control manager, after a
//! hidden PowerShell watcher was started that starts the service again once
//! this process ends and the service has stopped. The watcher lives in the
//! signed-in user's session: signing out of Windows ends it with this copy,
//! and the service (Automatic start) then runs again only from the next boot.
//! Before anything is done,
//! the process that answered `/instance` must be this program (`process`),
//! the one the system shows listening on the address that answered
//! (`listener`), and for a service, the child of that service's own process.
//!
//! Only `startup` calls this, and only for an interactive run on a loopback
//! bind: `/instance` answers nothing else.

use std::time::{Duration, Instant};

use super::{InstanceInfo, RunKind, STOP_HEADER, STOP_ROUTE, listener, probe, process::Process};
use crate::infrastructure::{console, listeners};

/// How long a console copy gets to stop after its stop request.
const STOP_WAIT: Duration = Duration::from_secs(10);
/// How long a terminated process gets to go away.
const TERMINATE_WAIT: Duration = Duration::from_secs(5);
/// How long ports and the data folder may take to come free once the copy
/// holding them has gone.
pub const RELEASE_WAIT: Duration = Duration::from_secs(5);
/// How long a Windows service gets to stop.
#[cfg_attr(not(windows), allow(dead_code))]
const SERVICE_WAIT: Duration = Duration::from_secs(30);

/// What may be done about a copy holding the port or data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Nothing: report it (no one at the console, or a dev server).
    Refuse,
    /// Offer to stop the console copy.
    StopConsole,
    /// Offer to stop the Windows service of that name.
    StopService(String),
    /// A service on another system: its service manager must stop it.
    ServiceElsewhere(String),
}

/// What may be done about `info` for this run. `windows` is the platform,
/// injected for tests.
pub fn plan(info: &InstanceInfo, interactive: bool, windows: bool) -> Plan {
    if !interactive {
        return Plan::Refuse;
    }
    match &info.run {
        RunKind::Dev => Plan::Refuse,
        RunKind::Console => Plan::StopConsole,
        RunKind::Service { name } if windows => Plan::StopService(name.clone()),
        RunKind::Service { name } => Plan::ServiceElsewhere(name.clone()),
    }
}

/// How an offer to take over ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Takeover {
    /// The copy has stopped: take its place, trying until `retry_until`.
    Stopped { retry_until: Instant },
    /// Leave it running and use it (the answer was no, or it cannot be stopped).
    UseRunning,
    /// The process that answered is not confirmed to be this app: report the
    /// conflict as another program's.
    NotOurs,
    /// It was asked to stop but is still there; the text says what to do.
    Failed(String),
}

/// Whether the system shows `info.pid` as the process listening on the
/// address `info` names (the one that answered): a program answering
/// `/instance` with another process's id is not that process.
fn listens_where_it_answered(info: &InstanceInfo) -> bool {
    info.socket_address()
        .is_some_and(|address| listener::owned_by(info.pid, address))
}

/// Offers to stop a console copy, and stops it on yes.
pub async fn offer_console(info: &InstanceInfo) -> Takeover {
    // Opened first: on Windows the held handle keeps the id from being
    // reused between this check and the end.
    let Some(process) = Process::open_ours(info.pid) else {
        return Takeover::NotOurs;
    };
    if !listens_where_it_answered(info) {
        return Takeover::NotOurs;
    }
    println!(
        "Listening Exam Generator {} is already running at {} (PID {}, {}).",
        info.version,
        info.url(),
        info.pid,
        info.config_dir
    );
    if !console::ask_yes_no("Stop it and start this copy instead? [y/N] ") {
        return Takeover::UseRunning;
    }
    println!("Stopping the running copy (PID {})...", info.pid);
    match request_stop(info).await {
        Ok(()) => {
            if process.wait_exit(STOP_WAIT).await {
                return stopped();
            }
            tracing::warn!(
                "PID {} did not stop within 10 s of its stop request",
                info.pid
            );
        }
        Err(reason) => tracing::info!("Stop request to PID {} not sent: {reason}", info.pid),
    }
    // Unix ids are not held open: the copy must still answer as itself.
    #[cfg(unix)]
    {
        let same = match info.socket_address() {
            Some(address) => {
                matches!(probe(address).await, super::Occupant::Ours(now) if now.pid == info.pid)
                    && listener::owned_by(info.pid, address)
            }
            None => false,
        };
        if !same && !process.has_exited() {
            return Takeover::Failed(did_not_stop(info));
        }
    }
    if let Err(error) = process.terminate() {
        tracing::warn!("Cannot end PID {}: {error}", info.pid);
    }
    if process.wait_exit(TERMINATE_WAIT).await {
        stopped()
    } else {
        Takeover::Failed(did_not_stop(info))
    }
}

fn stopped() -> Takeover {
    Takeover::Stopped {
        retry_until: Instant::now() + RELEASE_WAIT,
    }
}

fn did_not_stop(info: &InstanceInfo) -> String {
    format!(
        "Listening Exam Generator (PID {}) did not stop. Close its window, then start this copy again.",
        info.pid
    )
}

/// Sends `POST /instance/stop` with the token from the copy's own
/// `instance.json`; `Ok` when it accepted. The token goes only to the address
/// the file itself names, for the process that wrote it.
async fn request_stop(info: &InstanceInfo) -> Result<(), String> {
    let address = info
        .socket_address()
        .filter(|address| address.ip().is_loopback())
        .ok_or("not on a loopback address")?;
    let file = super::read_file(std::path::Path::new(&info.data_dir))
        .filter(|file| file.info.pid == info.pid && file.info.socket_address() == Some(address))
        .ok_or("its instance.json is unreadable or another process's")?;
    let client = probe::local_client().map_err(|e| e.to_string())?;
    let response = client
        .post(format!("{}{STOP_ROUTE}", listeners::browser_url(address)))
        .header(STOP_HEADER, file.stop_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    match response.status().as_u16() {
        202 => Ok(()),
        status => Err(format!("answered {status}")),
    }
}

/// Offers to stop the Windows service `name` that runs `info`, and stops it on
/// yes; this copy then serves until it stops, and the service starts again.
#[cfg(windows)]
pub async fn offer_service(info: &InstanceInfo, name: &str) -> Takeover {
    use super::win32::{self, Service};

    if !our_service(info, name) {
        return Takeover::NotOurs;
    }
    let url = info.url();
    println!(
        "Listening Exam Generator {} is running as the Windows service {name} at {url}.",
        info.version
    );
    if !console::ask_yes_no(
        "Stop the service and use this copy instead? It starts again when this copy stops. [y/N] ",
    ) {
        return Takeover::UseRunning;
    }
    // The answer took a while: it must still be the same service and process.
    if !our_service(info, name) {
        return Takeover::NotOurs;
    }
    let service = match Service::open_control(name) {
        Ok(service) => service,
        Err(error) if win32::access_denied(&error) => {
            println!("{}", no_right_to_stop(name));
            return Takeover::UseRunning;
        }
        Err(error) => {
            tracing::warn!("Cannot open the service {name}: {error}");
            return Takeover::NotOurs;
        }
    };
    // First the watcher: if it cannot start, the service is left running
    // rather than stopped with nothing to start it again.
    if let Err(error) = spawn_watcher(name) {
        println!(
            "Cannot start the helper that starts the service {name} again later ({error}), so the service was left running."
        );
        return Takeover::UseRunning;
    }
    let keep_service = || {
        println!("The service did not stop; using it at {url}.");
        Takeover::UseRunning
    };
    if let Err(error) = service.stop() {
        tracing::warn!("Cannot stop the service {name}: {error}");
        return keep_service();
    }
    println!("Stopping the service {name}...");
    let deadline = Instant::now() + SERVICE_WAIT;
    loop {
        if service.state().is_ok_and(|state| state.stopped) {
            tracing::info!("Service {name} stopped; it starts again when this copy stops");
            return Takeover::Stopped {
                retry_until: deadline.max(Instant::now() + RELEASE_WAIT),
            };
        }
        if Instant::now() >= deadline {
            return keep_service();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// What an account without the right to stop the service `name` is told;
/// this copy then uses the running one.
#[cfg_attr(not(windows), allow(dead_code))]
fn no_right_to_stop(name: &str) -> String {
    format!(
        "You do not have the right to stop the service {name}. Run this copy as an administrator, or ask whoever installed the service to give your account the right to stop and start it."
    )
}

/// Whether `info.pid` is this program, started by the running service `name`
/// (NSSM's process is the service's, the app is its child), and listening
/// where `info` answered. Toolhelp and the listener table give all of it
/// without any right on the service's processes.
#[cfg(windows)]
fn our_service(info: &InstanceInfo, name: &str) -> bool {
    use super::{process, win32};

    let pid = info.pid;
    if !crate::infrastructure::config::valid_service_name(name)
        || !process::plausible_pid(pid, std::process::id())
    {
        return false;
    }
    let Ok(state) = win32::Service::open_query(name).and_then(|service| service.state()) else {
        return false;
    };
    if !state.running || state.pid == 0 {
        return false;
    }
    let Ok(own) = std::env::current_exe() else {
        return false;
    };
    matches!(
        win32::process_entry(pid),
        Ok(Some(entry)) if entry.parent == state.pid && process::same_program(&own, &entry.exe)
    ) && listens_where_it_answered(info)
}

/// Starts the hidden watcher that starts the service again once this process
/// has ended and the service has stopped. It has no console and no stdio of
/// ours, survives our window closing (not signing out: the session ends it),
/// and never sees the API key.
#[cfg(windows)]
fn spawn_watcher(name: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};

    // Never looked up on PATH or next to server.exe.
    let powershell = super::win32::system_directory()?
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let script = watcher_script(std::process::id(), name);
    Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden"])
        .arg("-EncodedCommand")
        .arg(encoded_command(&script))
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("GEMINI_API_KEY")
        .spawn()
        .map(drop)
}

/// The watcher: wait for `pid` to end (through a handle on it, so a reused
/// id is never waited on), then for the service to finish stopping (at most
/// two minutes: a service still stopping cannot be started), then start it
/// if it is stopped. `name` must have passed `valid_service_name` (no quote
/// can end the string).
#[cfg_attr(not(windows), allow(dead_code))]
fn watcher_script(pid: u32, name: &str) -> String {
    format!(
        "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; if ($p) {{ $p.WaitForExit() }}; \
$s = Get-Service -Name '{name}' -ErrorAction SilentlyContinue; if ($s) {{ \
try {{ $s.WaitForStatus('Stopped', [TimeSpan]::FromMinutes(2)) }} catch {{ }}; $s.Refresh(); \
if ($s.Status -eq 'Stopped') {{ Start-Service -Name '{name}' }} }}"
    )
}

/// PowerShell's `-EncodedCommand`: the script as UTF-16LE, in base64. Nothing
/// in it needs quoting on the command line.
#[cfg_attr(not(windows), allow(dead_code))]
fn encoded_command(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::super::tests::info;
    use super::*;

    #[test]
    fn only_an_interactive_run_offers_to_stop_a_copy() {
        let service = RunKind::Service {
            name: "VMQ-MVP".into(),
        };
        for windows in [false, true] {
            for run in [RunKind::Console, RunKind::Dev, service.clone()] {
                assert_eq!(plan(&info(run), false, windows), Plan::Refuse);
            }
            assert_eq!(plan(&info(RunKind::Dev), true, windows), Plan::Refuse);
            assert_eq!(
                plan(&info(RunKind::Console), true, windows),
                Plan::StopConsole
            );
        }
        assert_eq!(
            plan(&info(service.clone()), true, true),
            Plan::StopService("VMQ-MVP".into())
        );
        assert_eq!(
            plan(&info(service), true, false),
            Plan::ServiceElsewhere("VMQ-MVP".into())
        );
    }

    #[test]
    fn encoded_commands_are_utf16le_base64() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("dir"))
        assert_eq!(encoded_command("dir"), "ZABpAHIA");
        assert_eq!(encoded_command(""), "");
        // Non-ASCII stays one UTF-16 unit per character.
        assert_eq!(encoded_command("é"), "6QA=");
        // Only base64 characters: safe on any command line.
        let encoded = encoded_command(&watcher_script(4242, "VMQ-MVP"));
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        );
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&units).unwrap(),
            watcher_script(4242, "VMQ-MVP")
        );
    }

    #[test]
    fn the_watcher_waits_for_this_process_and_the_stop_then_starts_the_service() {
        assert_eq!(
            watcher_script(4242, "VMQ-MVP"),
            "$p = Get-Process -Id 4242 -ErrorAction SilentlyContinue; if ($p) { $p.WaitForExit() }; \
$s = Get-Service -Name 'VMQ-MVP' -ErrorAction SilentlyContinue; if ($s) { \
try { $s.WaitForStatus('Stopped', [TimeSpan]::FromMinutes(2)) } catch { }; $s.Refresh(); \
if ($s.Status -eq 'Stopped') { Start-Service -Name 'VMQ-MVP' } }"
        );
    }

    #[test]
    fn an_account_without_the_right_is_told_whom_to_ask() {
        assert_eq!(
            no_right_to_stop("VMQ-MVP"),
            "You do not have the right to stop the service VMQ-MVP. Run this copy as an administrator, or ask whoever installed the service to give your account the right to stop and start it."
        );
    }

    #[tokio::test]
    async fn a_stop_request_needs_the_token_of_the_copy_that_answered() {
        use dioxus::server::axum::{
            Router,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        use std::sync::{Arc, Mutex};
        let seen = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
        let record = seen.clone();
        let router = Router::new().route(
            STOP_ROUTE,
            post(move |headers: HeaderMap| {
                let record = record.clone();
                async move {
                    let token = headers
                        .get(STOP_HEADER)
                        .map(|value| value.to_str().unwrap().to_owned());
                    let accepted = token.as_deref() == Some("token-1234");
                    record.lock().unwrap().push(token);
                    match accepted {
                        true => StatusCode::ACCEPTED,
                        false => StatusCode::FORBIDDEN,
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            dioxus::server::axum::serve(listener, router).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let running = InstanceInfo {
            address: address.to_string(),
            data_dir: dir.path().display().to_string(),
            ..info(RunKind::Console)
        };
        // No instance.json: nothing is sent.
        assert!(request_stop(&running).await.is_err());
        let write = |pid, token: &str| {
            let file = super::super::InstanceFile {
                info: InstanceInfo {
                    pid,
                    ..running.clone()
                },
                stop_token: token.into(),
            };
            super::super::write_file(dir.path(), &file).unwrap();
        };
        // Another process's file: not sent either.
        write(running.pid + 1, "token-1234");
        assert!(request_stop(&running).await.is_err());
        // A file naming another address: its token belongs to the copy there.
        let file = super::super::InstanceFile {
            info: InstanceInfo {
                address: "127.0.0.1:9".into(),
                ..running.clone()
            },
            stop_token: "token-1234".into(),
        };
        super::super::write_file(dir.path(), &file).unwrap();
        assert!(request_stop(&running).await.is_err());
        assert!(seen.lock().unwrap().is_empty());
        write(running.pid, "token-1234");
        assert_eq!(request_stop(&running).await, Ok(()));
        write(running.pid, "stale");
        assert_eq!(
            request_stop(&running).await,
            Err("answered 403".to_string())
        );
        assert_eq!(
            *seen.lock().unwrap(),
            [Some("token-1234".to_string()), Some("stale".to_string())]
        );
        // Never to an address off this computer.
        let remote = InstanceInfo {
            address: "192.0.2.1:8080".into(),
            ..running.clone()
        };
        assert!(request_stop(&remote).await.is_err());
        server.abort();
    }
}
