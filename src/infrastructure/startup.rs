//! Portable lifecycle, with explicit errors before opening a browser.
//!
//! The release path takes the data-folder lock, binds the main address and,
//! with `PUBLIC_PORT`, the published port for Cloudflare Tunnel, and only then
//! freezes the configuration, opens the job database, fails the recordings a
//! stopped server left unfinished and serves one router on both (`listeners`)
//! until Ctrl+C, Ctrl+Break, SIGTERM or `POST /instance/stop`. A port or data
//! folder another copy of the app holds is reported with that copy's version
//! and process id (`instance`); a person at the console may instead stop that
//! copy and take its place, or use it (`instance::takeover`). Under
//! `dx serve` only the main address is served.
use std::{path::Path, time::Instant};

use super::{
    config::{AppConfig, StartupOptions, config},
    instance::{
        self, DataLock, InstanceInfo, Occupant, RunKind,
        takeover::{self, Plan, Takeover},
    },
    jobs,
    listeners::{self, BindError, Listener},
};
use dioxus::prelude::*;
use tokio::net::TcpListener;

/// Every route of the app, built once per process: the published listener
/// serves a clone of it.
fn router(app: fn() -> Element) -> dioxus::server::axum::Router {
    use dioxus::server::axum::routing::{get, post};
    dioxus::server::router(app)
        .route(
            crate::application::audio::AUDIO_ROUTE,
            get(jobs::serve_audio),
        )
        .route(
            crate::application::voices::VOICE_SAMPLE_ROUTE,
            get(jobs::serve_voice_sample),
        )
        .route(instance::INFO_ROUTE, get(instance::serve_info))
        .route(instance::STOP_ROUTE, post(instance::stop))
}

pub fn run(app: fn() -> Element) -> Result<(), String> {
    let options = StartupOptions::parse(std::env::args_os().skip(1))?;
    let (cfg, _log_guard) = super::prepare(&options)?;
    check_browser_assets()?;
    if !options.portable && cfg!(debug_assertions) {
        return serve_dev(app, cfg, &options);
    }
    // Built after `prepare`: its `set_var` must run before any other thread.
    let runtime =
        tokio::runtime::Runtime::new().map_err(|e| format!("Cannot start runtime: {e}"))?;
    let bound = match runtime.block_on(acquire_and_bind(&cfg, &options))? {
        Acquired::Bound(bound) => bound,
        Acquired::UseRunning(url) => {
            use_running_copy(&url, &options);
            return Ok(());
        }
    };
    super::finish(cfg, &options, Some(&runtime))?;
    let served = runtime.block_on(serve(app, &options, bound));
    // Connections cut at the drain deadline, or a blocking file task, must not
    // hold the process: give them a moment, then exit.
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    served
}

fn check_browser_assets() -> Result<(), String> {
    let public = std::env::var_os("DIOXUS_PUBLIC_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap_or_default()
                .with_file_name("public")
        });
    let index = public.join("index.html");
    let contents = std::fs::read_to_string(&index)
        .map_err(|e| format!("Cannot read browser assets {}: {e}", index.display()))?;

    if !contents.contains("id=\"main\"")
        || !contents.contains("</head>")
        || !contents.contains("</body>")
    {
        return Err(format!("Invalid browser index {}", index.display()));
    }
    Ok(())
}

/// `dx serve`: dioxus binds the main address itself (and panics when it is
/// taken), so nothing is identified here; the data-folder lock is tried once.
fn serve_dev(app: fn() -> Element, cfg: AppConfig, options: &StartupOptions) -> Result<(), String> {
    let owns_data = match instance::lock_data_folder(&cfg.data_dir)? {
        DataLock::Held => true,
        DataLock::InUse => {
            notice("Another copy of the app uses this data folder; its recordings are left alone.");
            false
        }
        DataLock::Unsupported(error) => {
            notice(&lock_unsupported(&cfg.data_dir, &error));
            false
        }
    };
    // `dx serve` never asks at the console, so no runtime for a key check.
    super::finish(cfg, options, None)?;
    if config().public_address.is_some() {
        notice(
            "PUBLIC_PORT is ignored under dx serve; use a release or --portable run to test the tunnel port.",
        );
    }
    // The callback runs again on every hot-patch; announcing and sweeping
    // happen once, or this server's own recordings would be failed.
    static STARTED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    dioxus::serve(move || async move {
        if let Err(error) = jobs::JobStore::initialize().await {
            eprintln!("ERROR: {error}");
            std::process::exit(1);
        }
        STARTED
            .get_or_init(|| announce(RunKind::Dev, owns_data))
            .await;
        jobs::ensure_cleanup_running();
        Ok(router(app))
    });
}

/// The listeners of a release run, bound while holding the data folder.
struct Bound {
    main: TcpListener,
    published: Option<TcpListener>,
    /// This process holds the data-folder lock, so it may fail recordings an
    /// earlier server left unfinished and write `instance.json`.
    owns_data: bool,
}

/// How `acquire_and_bind` ended.
enum Acquired {
    /// This process holds the data folder and every configured port.
    Bound(Bound),
    /// Another copy keeps running and this one gives way to it (its URL).
    UseRunning(String),
}

/// Why the data folder or a port could not be taken (yet).
enum Conflict {
    /// Another process holds the data-folder lock.
    DataFolder,
    /// Another process listens on the main port.
    Port(BindError),
    /// Another process listens on `PUBLIC_PORT`. Never taken over: a published
    /// listener does not answer `/instance`. When the main port's copy is
    /// stopped, the same copy has released both.
    Published(BindError),
    /// Anything else.
    Failed(String),
}

/// At most this many copies are stopped in one start (one holding the data
/// folder, one the port, and a spare): more would be a loop.
const MAX_TAKEOVERS: usize = 3;

/// Takes the data folder, then the ports. When another copy of the app holds
/// either, the error names it (`instance::probe`); at an interactive console
/// the person may stop that copy and take its place, or use it instead
/// (`instance::takeover`). Done means the lock is held and every configured
/// port is bound.
async fn acquire_and_bind(cfg: &AppConfig, options: &StartupOptions) -> Result<Acquired, String> {
    let interactive = options.interactive(false);
    let mut lock_warned = false;
    let mut stopped: Vec<u32> = Vec::new();
    let mut attempt = try_acquire(cfg, &mut lock_warned).await;
    loop {
        let conflict = match attempt {
            Ok(bound) => return Ok(Acquired::Bound(bound)),
            Err(conflict) => conflict,
        };
        let occupant = match &conflict {
            Conflict::DataFolder => data_folder_occupant(&cfg.data_dir).await,
            Conflict::Port(error) => instance::probe(error.address).await,
            Conflict::Published(error) => return Err(error.to_string()),
            Conflict::Failed(text) => return Err(text.clone()),
        };
        let refusal = |occupant: &Occupant| conflict_message(cfg, &conflict, occupant);
        let Occupant::Ours(info) = &occupant else {
            return Err(refusal(&occupant));
        };
        // A copy stopped once and still answering is not stopped again.
        if stopped.contains(&info.pid) || stopped.len() >= MAX_TAKEOVERS {
            return Err(refusal(&occupant));
        }
        let outcome = match takeover::plan(info, interactive, cfg!(windows)) {
            Plan::Refuse => return Err(refusal(&occupant)),
            Plan::ServiceElsewhere(name) => {
                return Err(service_elsewhere(&cfg.data_dir, &conflict, info, &name));
            }
            Plan::StopConsole => takeover::offer_console(info).await,
            #[cfg(windows)]
            Plan::StopService(name) => takeover::offer_service(info, &name).await,
            #[cfg(not(windows))]
            Plan::StopService(_) => return Err(refusal(&occupant)),
        };
        match outcome {
            Takeover::Stopped { retry_until } => {
                stopped.push(info.pid);
                attempt = retry(cfg, &mut lock_warned, retry_until).await;
            }
            Takeover::UseRunning => return Ok(Acquired::UseRunning(info.url())),
            Takeover::NotOurs => return Err(refusal(&Occupant::Unknown)),
            Takeover::Failed(text) => return Err(text),
        }
    }
}

/// One try at the data folder and the ports. A lock the file system cannot
/// take is reported once (`lock_warned`).
async fn try_acquire(cfg: &AppConfig, lock_warned: &mut bool) -> Result<Bound, Conflict> {
    let owns_data = match instance::lock_data_folder(&cfg.data_dir).map_err(Conflict::Failed)? {
        DataLock::Held => true,
        DataLock::InUse => return Err(Conflict::DataFolder),
        DataLock::Unsupported(error) => {
            if !std::mem::replace(lock_warned, true) {
                notice(&lock_unsupported(&cfg.data_dir, &error));
            }
            false
        }
    };
    let main = listeners::bind(Listener::Main, cfg.address)
        .await
        .map_err(|error| match error.in_use() {
            true => Conflict::Port(error),
            false => Conflict::Failed(error.to_string()),
        })?;
    let published = match cfg.public_address {
        Some(address) => Some(
            listeners::bind(Listener::Published, address)
                .await
                .map_err(|error| match error.in_use() {
                    true => Conflict::Published(error),
                    false => Conflict::Failed(error.to_string()),
                })?,
        ),
        None => None,
    };
    Ok(Bound {
        main,
        published,
        owns_data,
    })
}

/// Tries again until `until` while a stopped copy's lock or ports are still
/// being released; the conflict left after that is resolved like the first.
async fn retry(cfg: &AppConfig, lock_warned: &mut bool, until: Instant) -> Result<Bound, Conflict> {
    loop {
        let attempt = try_acquire(cfg, lock_warned).await;
        let busy = matches!(
            attempt,
            Err(Conflict::DataFolder | Conflict::Port(_) | Conflict::Published(_))
        );
        if !busy || Instant::now() >= until {
            return attempt;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Who holds the data folder: the copy that wrote `instance.json` there, if
/// it still answers at its address with the same process id.
async fn data_folder_occupant(data_dir: &Path) -> Occupant {
    let written = instance::read_file(data_dir)
        .and_then(|file| Some((file.info.socket_address()?, file.info.pid)));
    match written {
        Some((address, pid)) => match instance::probe(address).await {
            Occupant::Ours(info) if info.pid == pid => Occupant::Ours(info),
            _ => Occupant::Unknown,
        },
        None => Occupant::Unknown,
    }
}

/// The failure for `conflict` when nothing more is done about `occupant`.
fn conflict_message(cfg: &AppConfig, conflict: &Conflict, occupant: &Occupant) -> String {
    match conflict {
        Conflict::DataFolder => data_folder_message(&cfg.data_dir, occupant),
        Conflict::Port(error) => port_in_use(error, occupant),
        Conflict::Published(error) => error.to_string(),
        Conflict::Failed(text) => text.clone(),
    }
}

/// A service copy outside Windows: only its service manager may stop it.
fn service_elsewhere(
    data_dir: &Path,
    conflict: &Conflict,
    info: &InstanceInfo,
    name: &str,
) -> String {
    let what = format!(
        "Listening Exam Generator {} runs as the service {name}",
        info.version
    );
    let stop = format!(
        "stop it with your service manager (for example: systemctl stop {name}) and start this copy again."
    );
    match conflict {
        Conflict::Port(error) => format!("Cannot listen on {}: {what}; {stop}", error.address),
        _ => format!(
            "{what} and uses the data folder {}; {stop}",
            data_dir.display()
        ),
    }
}

/// Gives way to the copy already running at `url`. The browser opens before
/// this process exits: a thread opening it would end with the process.
fn use_running_copy(url: &str, options: &StartupOptions) {
    println!("Using the copy that is already running: {url}");
    // Windows: `cmd /c start` returns at once, so wait for it. Elsewhere the
    // opener may wait for the browser itself: start it in a session of its
    // own (`detach`) and leave.
    if options.opens_browser() && !open_browser(url, cfg!(windows)) {
        eprintln!("Could not open the browser. Open {url} manually.");
    }
}

/// Opens `url` in the default browser, without passing AppImage libraries
/// into unrelated programs. `wait`: until the opener finishes (its exit status
/// counts); otherwise only until it started.
fn open_browser(url: &str, wait: bool) -> bool {
    use std::process::Stdio;
    open::commands(url).into_iter().any(|mut command| {
        command
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("DIOXUS_PUBLIC_PATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if wait {
            command.status().is_ok_and(|status| status.success())
        } else {
            detach(&mut command);
            command.spawn().is_ok()
        }
    })
}

/// Starts a command this process does not wait for in a session of its own:
/// when this process leads the terminal's session (a desktop entry with
/// `Terminal=true`) and exits right after, the terminal's hangup (SIGHUP)
/// goes to its process group and would end the opener before the browser
/// opens. Nothing to do elsewhere.
fn detach(command: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: runs in the child between fork and exec, where only
        // async-signal-safe calls are allowed; setsid is one (POSIX), and the
        // closure touches nothing else but errno.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

fn data_folder_message(data_dir: &Path, occupant: &Occupant) -> String {
    let folder = data_dir.display();
    match occupant {
        Occupant::Ours(info) if info.run == RunKind::Dev => format!(
            "A development server (dx serve) is already using the data folder {folder} at {}; stop it first.",
            info.url()
        ),
        Occupant::Ours(info) => format!(
            "Listening Exam Generator {} (PID {}) is already using the data folder {folder} at {}. Stop it first.",
            info.version,
            info.pid,
            info.url()
        ),
        Occupant::Unknown => format!(
            "Another program is using the data folder {folder} (its lock is held). Stop it first."
        ),
    }
}

fn port_in_use(error: &BindError, occupant: &Occupant) -> String {
    let address = error.address;
    match occupant {
        Occupant::Ours(info) if info.run == RunKind::Dev => format!(
            "Cannot listen on {address}: a development server (dx serve) is running there; stop it first."
        ),
        Occupant::Ours(info) => {
            let service = match &info.run {
                RunKind::Service { name } if cfg!(windows) => {
                    format!(", as the Windows service {name}")
                }
                RunKind::Service { name } => format!(", as the service {name}"),
                _ => String::new(),
            };
            format!(
                "Cannot listen on {address}: Listening Exam Generator {} is already running there (PID {}{service}). Stop it first or choose another PORT.",
                info.version, info.pid
            )
        }
        Occupant::Unknown => error.to_string(),
    }
}

fn lock_unsupported(data_dir: &Path, error: &std::io::Error) -> String {
    format!(
        "The data folder {} cannot be locked ({error}), so recordings an earlier stop interrupted are left as they are.",
        data_dir.display()
    )
}

/// Prints and logs something the operator should know that does not stop startup.
fn notice(text: &str) {
    println!("Note: {text}");
    tracing::warn!("{text}");
}

/// Makes this process known (the instance routes and, when it owns the data
/// folder, `instance.json`) and fails the recordings a stopped server left
/// pending or processing: no task makes them any more. Without the
/// data-folder lock they may be another live server's, so they are left alone.
async fn announce(run: RunKind, owns_data: bool) {
    let cfg = config();
    let info = InstanceInfo::of_this_process(cfg, run);
    if let Err(error) = instance::announce(info, owns_data.then_some(cfg.data_dir.as_path())) {
        notice(&error);
    }
    if !owns_data {
        return;
    }
    let store = jobs::JobStore::global().await;
    match store.fail_interrupted(jobs::INTERRUPTED_MESSAGE).await {
        Ok(0) => {}
        Ok(count) => {
            tracing::info!("{count} recording(s) interrupted by the last stop marked as failed")
        }
        Err(e) => tracing::warn!("Cannot mark interrupted recordings as failed: {e}"),
    }
}

/// Serves a release run on the listeners `acquire_and_bind` returned.
async fn serve(app: fn() -> Element, options: &StartupOptions, bound: Bound) -> Result<(), String> {
    jobs::JobStore::initialize().await?;
    let run = match &options.service {
        Some(name) => RunKind::Service { name: name.clone() },
        None => RunKind::Console,
    };
    announce(run, bound.owns_data).await;
    let app_router = router(app);
    jobs::ensure_cleanup_running();
    listeners::stop_on_signals();
    let url = listeners::browser_url(config().address);
    println!("Listening Exam Generator: {url}");
    if let Some(address) = config().public_address {
        println!(
            "Published port (Cloudflare Tunnel): {}",
            listeners::browser_url(address)
        );
    }
    if options.service.is_none() {
        println!("Press Ctrl+C to stop.");
    }
    if options.opens_browser() {
        let browser_url = url.clone();
        // A desktop opener may wait for its browser. Do not make runtime shutdown
        // wait for it.
        std::thread::spawn(move || {
            if !open_browser(&browser_url, true) {
                eprintln!("Could not open the browser. Open {browser_url} manually.");
            }
        });
    }
    listeners::serve(
        app_router,
        bound.main,
        bound.published,
        listeners::shutdown_requests(),
    )
    .await?;
    println!("Server stopped cleanly.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(run: RunKind) -> Occupant {
        Occupant::Ours(InstanceInfo {
            app: instance::APP_ID.into(),
            version: "0.9.0".into(),
            pid: 4242,
            run,
            address: "127.0.0.1:8080".into(),
            public_port: None,
            config_dir: "app".into(),
            data_dir: "app/data".into(),
        })
    }

    fn taken() -> BindError {
        BindError {
            listener: Listener::Main,
            address: "127.0.0.1:8080".parse().unwrap(),
            error: std::io::Error::from(std::io::ErrorKind::AddrInUse),
        }
    }

    #[test]
    fn a_taken_port_names_the_copy_holding_it() {
        assert_eq!(
            port_in_use(&taken(), &running(RunKind::Console)),
            "Cannot listen on 127.0.0.1:8080: Listening Exam Generator 0.9.0 is already running there (PID 4242). Stop it first or choose another PORT."
        );
        let service = port_in_use(
            &taken(),
            &running(RunKind::Service {
                name: "VMQ-MVP".into(),
            }),
        );
        assert!(service.contains("(PID 4242, as the "), "{service}");
        assert!(
            service.contains("service VMQ-MVP). Stop it first"),
            "{service}"
        );
        assert_eq!(
            port_in_use(&taken(), &running(RunKind::Dev)),
            "Cannot listen on 127.0.0.1:8080: a development server (dx serve) is running there; stop it first."
        );
        // Another program: the message from before, naming the setting.
        let other = port_in_use(&taken(), &Occupant::Unknown);
        assert_eq!(other, taken().to_string());
        assert!(other.starts_with("Cannot listen on 127.0.0.1:8080: "));
    }

    #[test]
    fn a_service_outside_windows_is_left_to_its_service_manager() {
        let Occupant::Ours(info) = running(RunKind::Service {
            name: "listening".into(),
        }) else {
            unreachable!()
        };
        let stop = "stop it with your service manager (for example: systemctl stop listening) and start this copy again.";
        assert_eq!(
            service_elsewhere(
                Path::new("data"),
                &Conflict::Port(taken()),
                &info,
                "listening"
            ),
            format!(
                "Cannot listen on 127.0.0.1:8080: Listening Exam Generator 0.9.0 runs as the service listening; {stop}"
            )
        );
        assert_eq!(
            service_elsewhere(Path::new("data"), &Conflict::DataFolder, &info, "listening"),
            format!(
                "Listening Exam Generator 0.9.0 runs as the service listening and uses the data folder {}; {stop}",
                Path::new("data").display()
            )
        );
    }

    #[test]
    fn a_held_data_folder_names_the_copy_holding_it() {
        let dir = Path::new("data");
        let folder = dir.display();
        assert_eq!(
            data_folder_message(dir, &running(RunKind::Console)),
            format!(
                "Listening Exam Generator 0.9.0 (PID 4242) is already using the data folder {folder} at http://127.0.0.1:8080. Stop it first."
            )
        );
        assert_eq!(
            data_folder_message(dir, &running(RunKind::Dev)),
            format!(
                "A development server (dx serve) is already using the data folder {folder} at http://127.0.0.1:8080; stop it first."
            )
        );
        assert_eq!(
            data_folder_message(dir, &Occupant::Unknown),
            format!(
                "Another program is using the data folder {folder} (its lock is held). Stop it first."
            )
        );
    }

    #[tokio::test]
    async fn a_held_data_folder_is_named_only_by_the_copy_that_wrote_instance_json() {
        use dioxus::server::axum::{Router, routing::get};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let pid = std::process::id() + 1;
        let Occupant::Ours(answer) = running(RunKind::Console) else {
            unreachable!()
        };
        let answer = InstanceInfo {
            pid,
            address: address.to_string(),
            ..answer
        };
        let body = serde_json::to_string(&answer).unwrap();
        let router = Router::new().route(instance::INFO_ROUTE, get(move || async move { body }));
        let server = tokio::spawn(async move {
            dioxus::server::axum::serve(listener, router).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let write = |pid| {
            let file = instance::InstanceFile {
                info: InstanceInfo {
                    pid,
                    ..answer.clone()
                },
                stop_token: "token".into(),
            };
            std::fs::write(
                dir.path().join("instance.json"),
                serde_json::to_string(&file).unwrap(),
            )
            .unwrap();
        };
        write(pid);
        let text = data_folder_message(dir.path(), &data_folder_occupant(dir.path()).await);
        assert!(
            text.starts_with(&format!(
                "Listening Exam Generator 0.9.0 (PID {pid}) is already using the data folder"
            )),
            "{text}"
        );
        assert!(text.contains(&format!("at http://{address}.")), "{text}");
        // A stale file: the copy answering at its address is not the one that wrote it.
        write(pid + 1);
        let text = data_folder_message(dir.path(), &data_folder_occupant(dir.path()).await);
        assert!(
            text.starts_with("Another program is using the data folder"),
            "{text}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn an_unreadable_instance_file_leaves_the_occupant_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let text = data_folder_message(dir.path(), &data_folder_occupant(dir.path()).await);
        assert!(
            text.starts_with("Another program is using the data folder"),
            "{text}"
        );
    }

    /// The opener left behind by "use the running copy" leads a session of
    /// its own, so the hangup of the terminal this process leads never
    /// reaches it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_detached_command_leads_its_own_session() {
        use std::process::{Command, Stdio};
        let mut command = Command::new("cat");
        command.arg("/proc/self/stat").stdout(Stdio::piped());
        detach(&mut command);
        let child = command.spawn().unwrap();
        let pid = child.id();
        let output = child.wait_with_output().unwrap();
        let stat = String::from_utf8(output.stdout).unwrap();
        // `pid (comm) state ppid pgrp session ...`
        let fields: Vec<&str> = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .collect();
        let (group, session): (u32, u32) = (fields[2].parse().unwrap(), fields[3].parse().unwrap());
        assert_eq!((group, session), (pid, pid), "{stat}");
        // SAFETY: plain call about this process.
        let own_session = unsafe { libc::getsid(0) };
        assert_ne!(i64::from(own_session), i64::from(session), "{stat}");
    }
}
