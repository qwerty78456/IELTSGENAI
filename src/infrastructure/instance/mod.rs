//! Which copy of the app is running here, so another copy started later can
//! recognise it instead of failing with a bare "address in use".
//!
//! A running server holds `DATA_DIR/instance.lock` for its whole life (`lock`),
//! writes who it is to `DATA_DIR/instance.json` and answers `GET /instance`
//! for browsers and programs on this computer only. `POST /instance/stop`
//! lets another copy on this computer stop it cleanly; it needs the stop
//! token from `instance.json`, which `GET /instance` never returns, and is
//! refused to web pages (any `Origin` header). `probe` asks a port who
//! answers there, and `takeover` stops that copy when the person at the
//! console agrees and the system confirms that the process it names is the
//! one listening there (`listener`).

mod listener;
mod lock;
mod probe;
mod process;
pub mod takeover;
#[cfg(windows)]
mod win32;

pub use lock::{DataLock, lock_data_folder};
pub use probe::{Occupant, probe};

use std::{
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use dioxus::server::axum::{
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use super::{
    config::AppConfig,
    ingress::{self, Origin},
    listeners,
};

/// Names this app in `GET /instance`, so a copy does not mistake another
/// program for itself.
pub const APP_ID: &str = "listening-exam-generator";
/// `GET`: who is running here (local requests only).
pub const INFO_ROUTE: &str = "/instance";
/// `POST`: stop this copy cleanly (local requests with the stop token only).
pub const STOP_ROUTE: &str = "/instance/stop";
/// The header carrying the stop token from `instance.json`.
pub const STOP_HEADER: &str = "x-listening-exam-generator-stop";
/// In `DATA_DIR`: the running copy's `InstanceInfo` and stop token.
const INSTANCE_FILE: &str = "instance.json";

/// How this copy was started.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RunKind {
    /// By hand: a console, the portable launcher.
    Console,
    /// By a service manager, with `--service NAME`.
    Service { name: String },
    /// Under `dx serve`.
    Dev,
}

/// What `GET /instance` answers and `instance.json` holds (with the token).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstanceInfo {
    /// Always `APP_ID`.
    pub app: String,
    pub version: String,
    pub pid: u32,
    pub run: RunKind,
    /// The main address (`IP:PORT`) as a `SocketAddr`.
    pub address: String,
    /// `PUBLIC_PORT`, when this copy serves the Cloudflare Tunnel port.
    pub public_port: Option<u16>,
    pub config_dir: String,
    pub data_dir: String,
}

impl InstanceInfo {
    /// This process, as configured.
    pub fn of_this_process(cfg: &AppConfig, run: RunKind) -> Self {
        let public_port = match run {
            // `dx serve` serves the main address only.
            RunKind::Dev => None,
            _ => cfg.public_address.map(|address| address.port()),
        };
        Self {
            app: APP_ID.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            run,
            address: cfg.address.to_string(),
            public_port,
            config_dir: cfg
                .dotenv_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
                .display()
                .to_string(),
            data_dir: cfg.data_dir.display().to_string(),
        }
    }

    /// The main address, when it parses.
    pub fn socket_address(&self) -> Option<SocketAddr> {
        self.address.parse().ok()
    }

    /// The URL a browser on this computer opens for this copy.
    pub fn url(&self) -> String {
        match self.socket_address() {
            Some(address) => listeners::browser_url(address),
            None => format!("http://{}", self.address),
        }
    }
}

/// `instance.json`: `InstanceInfo` plus the token `POST /instance/stop` needs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstanceFile {
    #[serde(flatten)]
    pub info: InstanceInfo,
    pub stop_token: String,
}

/// This process once it serves: what the instance routes answer with.
static RUNNING: OnceLock<InstanceFile> = OnceLock::new();

/// Makes this process known: the instance routes answer from now on and, when
/// `data_dir` is given (this process holds the data-folder lock),
/// `instance.json` is written there. Only the first call counts.
pub fn announce(info: InstanceInfo, data_dir: Option<&Path>) -> Result<(), String> {
    let running = RUNNING.get_or_init(|| InstanceFile {
        info,
        stop_token: uuid::Uuid::new_v4().to_string(),
    });
    match data_dir {
        Some(dir) => write_file(dir, running),
        None => Ok(()),
    }
}

fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INSTANCE_FILE)
}

/// Replaces `instance.json` in one step (a temporary file renamed over it), so
/// a reader never sees half of it.
fn write_file(data_dir: &Path, file: &InstanceFile) -> Result<(), String> {
    let path = file_path(data_dir);
    let cannot = |e: &dyn std::fmt::Display| format!("Cannot write {}: {e}", path.display());
    let mut temp = tempfile::NamedTempFile::new_in(data_dir).map_err(|e| cannot(&e))?;
    serde_json::to_writer_pretty(&mut temp, file).map_err(|e| cannot(&e))?;
    temp.flush().map_err(|e| cannot(&e))?;
    temp.persist(&path).map_err(|e| cannot(&e.error))?;
    Ok(())
}

/// The `instance.json` another copy wrote in `data_dir`, when it is readable.
pub fn read_file(data_dir: &Path) -> Option<InstanceFile> {
    let text = std::fs::read_to_string(file_path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// `GET /instance`.
pub async fn serve_info(request: Request) -> Response {
    let origin = ingress::of_parts(request.extensions(), request.headers());
    info_response(origin, RUNNING.get())
}

/// `POST /instance/stop`.
pub async fn stop(request: Request) -> Response {
    let origin = ingress::of_parts(request.extensions(), request.headers());
    stop_response(
        origin,
        request.headers(),
        RUNNING.get(),
        listeners::request_shutdown,
    )
}

fn info_response(origin: Origin, running: Option<&InstanceFile>) -> Response {
    let response = match running {
        Some(running) if origin.is_local() => match serde_json::to_string(&running.info) {
            Ok(body) => ([(header::CONTENT_TYPE, "application/json")], body).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        // Not this computer, or not serving yet: as if the route did not exist.
        _ => (StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    not_stored(response)
}

/// The checks of `POST /instance/stop`, in order; `request_stop` runs only
/// when every one passes.
fn stop_response(
    origin: Origin,
    headers: &HeaderMap,
    running: Option<&InstanceFile>,
    request_stop: impl FnOnce(),
) -> Response {
    let Some(running) = running.filter(|_| origin.is_local()) else {
        return not_stored((StatusCode::NOT_FOUND, "Not found").into_response());
    };
    // A web page always sends Origin with a POST; the other copy never does.
    let token_matches = headers
        .get(STOP_HEADER)
        .is_some_and(|token| same_token(token.as_bytes(), running.stop_token.as_bytes()));
    if headers.contains_key(header::ORIGIN) || !token_matches {
        return not_stored((StatusCode::FORBIDDEN, "Forbidden").into_response());
    }
    let refusal = match running.info.run {
        RunKind::Service { .. } => {
            Some("This copy runs as a Windows service; stop the service instead.")
        }
        RunKind::Dev => Some("This is a development server; stop dx serve instead."),
        RunKind::Console => None,
    };
    if let Some(refusal) = refusal {
        return not_stored((StatusCode::CONFLICT, refusal).into_response());
    }
    tracing::info!("Stop requested by another copy of the app");
    request_stop();
    not_stored((StatusCode::ACCEPTED, "Stopping").into_response())
}

/// Compares the whole token whatever differs first.
fn same_token(given: &[u8], expected: &[u8]) -> bool {
    given.len() == expected.len()
        && given
            .iter()
            .zip(expected)
            .fold(0u8, |differs, (a, b)| differs | (a ^ b))
            == 0
}

fn not_stored(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    pub(super) fn info(run: RunKind) -> InstanceInfo {
        InstanceInfo {
            app: APP_ID.into(),
            version: "0.9.0".into(),
            pid: 4242,
            run,
            address: "127.0.0.1:8080".into(),
            public_port: Some(8081),
            config_dir: "C:\\Listening Exam Generator".into(),
            data_dir: "C:\\Listening Exam Generator\\data".into(),
        }
    }

    fn running(run: RunKind) -> InstanceFile {
        InstanceFile {
            info: info(run),
            stop_token: "token-1234".into(),
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    async fn body(response: Response) -> String {
        let bytes = dioxus::server::axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn run_kinds_and_files_round_trip_as_json() {
        for (run, json) in [
            (RunKind::Console, r#"{"kind":"console"}"#),
            (
                RunKind::Service {
                    name: "VMQ-MVP".into(),
                },
                r#"{"kind":"service","name":"VMQ-MVP"}"#,
            ),
            (RunKind::Dev, r#"{"kind":"dev"}"#),
        ] {
            assert_eq!(serde_json::to_string(&run).unwrap(), json);
            assert_eq!(serde_json::from_str::<RunKind>(json).unwrap(), run);
            let file = running(run.clone());
            let text = serde_json::to_string(&file).unwrap();
            assert_eq!(serde_json::from_str::<InstanceFile>(&text).unwrap(), file);
            // The file is the info plus one field.
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["stop_token"], "token-1234");
            assert_eq!(value["app"], APP_ID);
            assert_eq!(
                serde_json::from_value::<InstanceInfo>(value).unwrap(),
                info(run)
            );
        }
    }

    #[test]
    fn instance_json_is_replaced_whole_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_file(dir.path()), None);
        let first = running(RunKind::Console);
        write_file(dir.path(), &first).unwrap();
        assert_eq!(read_file(dir.path()), Some(first));
        let second = running(RunKind::Dev);
        write_file(dir.path(), &second).unwrap();
        assert_eq!(read_file(dir.path()), Some(second));
        // Only instance.json is left: the temporary file was renamed.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["instance.json"]);
        std::fs::write(dir.path().join("instance.json"), "<html>").unwrap();
        assert_eq!(read_file(dir.path()), None);
    }

    #[test]
    fn urls_come_from_the_main_address() {
        let mut info = info(RunKind::Console);
        assert_eq!(info.url(), "http://127.0.0.1:8080");
        info.address = "[::1]:9000".into();
        assert_eq!(info.url(), "http://[::1]:9000");
        info.address = "0.0.0.0:8080".into();
        assert_eq!(info.url(), "http://127.0.0.1:8080");
    }

    #[tokio::test]
    async fn only_this_computer_sees_who_is_running() {
        let file = running(RunKind::Console);
        let response = info_response(Origin::Local, Some(&file));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let text = body(response).await;
        assert!(
            !text.contains("stop_token") && !text.contains("token-1234"),
            "{text}"
        );
        let shown: InstanceInfo = serde_json::from_str(&text).unwrap();
        assert_eq!(shown, file.info);
        for origin in [Origin::Published, Origin::Remote] {
            let response = info_response(origin, Some(&file));
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{origin:?}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(body(response).await, "Not found");
        }
        let response = info_response(Origin::Local, None);
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_another_copy_with_the_token_stops_a_console_run() {
        let file = running(RunKind::Console);
        let token = (STOP_HEADER, "token-1234");
        let stopped = Cell::new(0);
        let stop = || stopped.set(stopped.get() + 1);
        let status = |origin, pairs: &[(&str, &str)], file: &InstanceFile| {
            stop_response(origin, &headers(pairs), Some(file), stop).status()
        };

        // Not this computer: the route does not exist.
        assert_eq!(
            status(Origin::Remote, &[token], &file),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(Origin::Published, &[token], &file),
            StatusCode::NOT_FOUND
        );
        let response = stop_response(Origin::Local, &headers(&[token]), None, stop);
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        // A web page (any Origin), no token, a wrong or longer token.
        for pairs in [
            &[token, ("origin", "http://127.0.0.1:8080")][..],
            &[],
            &[(STOP_HEADER, "token-1235")],
            &[(STOP_HEADER, "token-12345")],
            &[(STOP_HEADER, "")],
        ] {
            assert_eq!(
                status(Origin::Local, pairs, &file),
                StatusCode::FORBIDDEN,
                "{pairs:?}"
            );
        }
        assert_eq!(stopped.get(), 0);
        // A service or a dev server is stopped another way.
        let service = running(RunKind::Service {
            name: "VMQ-MVP".into(),
        });
        let response = stop_response(Origin::Local, &headers(&[token]), Some(&service), stop);
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(response).await,
            "This copy runs as a Windows service; stop the service instead."
        );
        let dev = running(RunKind::Dev);
        let response = stop_response(Origin::Local, &headers(&[token]), Some(&dev), stop);
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(response).await,
            "This is a development server; stop dx serve instead."
        );
        assert_eq!(stopped.get(), 0);

        let response = stop_response(Origin::Local, &headers(&[token]), Some(&file), stop);
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(stopped.get(), 1);
    }
}
