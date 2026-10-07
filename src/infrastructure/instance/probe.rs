//! Asks a port on this computer who answers there: a copy of this app
//! (`Ours`) or anything else (`Unknown`: another program, a 0.8 copy whose
//! `/instance` is an HTML page, no answer within two seconds).

use std::{net::SocketAddr, time::Duration};

use super::{APP_ID, INFO_ROUTE, InstanceInfo, RunKind, process::plausible_pid};
use crate::infrastructure::{config::valid_service_name, listeners};

/// Who answered `GET /instance`.
#[derive(Debug, Clone, PartialEq)]
pub enum Occupant {
    /// A copy of this app, other than this process, said who it is.
    Ours(InstanceInfo),
    /// Anything else.
    Unknown,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const TIMEOUT: Duration = Duration::from_secs(2);

/// Probes `address`, which must be a loopback address: `/instance` answers
/// only requests from this computer, so another address would be `Unknown`
/// after a wasted wait.
pub async fn probe(address: SocketAddr) -> Occupant {
    if !address.ip().is_loopback() {
        return Occupant::Unknown;
    }
    let Ok(client) = local_client() else {
        return Occupant::Unknown;
    };
    let url = format!("{}{INFO_ROUTE}", listeners::browser_url(address));
    let Ok(response) = client.get(url).send().await else {
        return Occupant::Unknown;
    };
    let status = response.status().as_u16();
    match response.bytes().await {
        Ok(body) => parse(status, &body, address, std::process::id()),
        Err(_) => Occupant::Unknown,
    }
}

/// A client for another copy's instance routes on this computer. A system or
/// environment proxy would not reach this computer's port, and a redirect
/// would make another address answer for this one.
pub(super) fn local_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TIMEOUT)
        .build()
}

/// What a `/instance` answer from `asked` says. `Ours` only for a 200 with
/// this app's JSON, a process id that could be another live process and, for
/// a copy that could be stopped, `asked` as its own address: a copy listens
/// where it says, so a program answering for another address is not it (and
/// must not get that copy's stop token or process). A dev server is exempt
/// (`dx serve` proxies its port to another one): nothing is done to it.
/// A service name must be one `--service` accepts. The texts that may be
/// printed lose their control characters (newlines, escape sequences): the
/// answer is not trusted to draw on the console.
fn parse(status: u16, body: &[u8], asked: SocketAddr, own_pid: u32) -> Occupant {
    if status != 200 {
        return Occupant::Unknown;
    }
    match serde_json::from_slice::<InstanceInfo>(body) {
        Ok(info)
            if info.app == APP_ID
                && plausible_pid(info.pid, own_pid)
                && info.socket_address().is_some()
                && (info.run == RunKind::Dev || info.socket_address() == Some(asked))
                && match &info.run {
                    RunKind::Service { name } => valid_service_name(name),
                    RunKind::Console | RunKind::Dev => true,
                } =>
        {
            Occupant::Ours(InstanceInfo {
                version: printable(&info.version),
                config_dir: printable(&info.config_dir),
                data_dir: printable(&info.data_dir),
                ..info
            })
        }
        _ => Occupant::Unknown,
    }
}

/// `text` without control characters.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::info;
    use super::*;

    /// The address `tests::info` names.
    fn asked() -> SocketAddr {
        "127.0.0.1:8080".parse().unwrap()
    }

    fn json(info: &InstanceInfo) -> Vec<u8> {
        serde_json::to_vec(info).unwrap()
    }

    #[test]
    fn only_this_apps_json_from_another_process_is_ours() {
        let own = 99;
        let parse = |status, body: &[u8], own| parse(status, body, asked(), own);
        let console = info(RunKind::Console);
        assert_eq!(
            parse(200, &json(&console), own),
            Occupant::Ours(console.clone())
        );
        // Fields this version does not know are fine; a newer copy may add some.
        let mut value = serde_json::to_value(&console).unwrap();
        value["extra"] = "later".into();
        assert_eq!(
            parse(200, value.to_string().as_bytes(), own),
            Occupant::Ours(console.clone())
        );
        let service = info(RunKind::Service {
            name: "VMQ-MVP".into(),
        });
        assert_eq!(parse(200, &json(&service), own), Occupant::Ours(service));

        let unknown = |status, body: &[u8]| parse(status, body, own) == Occupant::Unknown;
        // Another program, or a 0.8 copy answering with its HTML page.
        let mut other = console.clone();
        other.app = "something-else".into();
        assert!(unknown(200, &json(&other)));
        assert!(unknown(
            200,
            b"<!DOCTYPE html><html><body>app</body></html>"
        ));
        assert!(unknown(200, b""));
        assert!(unknown(200, br#"{"app":"listening-exam-generator"}"#));
        assert!(unknown(404, b"Not found"));
        assert!(unknown(404, &json(&console)));
        assert!(unknown(500, &json(&console)));
        // Process ids no other copy can have.
        for pid in [0, 1, own, i32::MAX as u32 + 1, u32::MAX] {
            let mut bad = console.clone();
            bad.pid = pid;
            assert!(unknown(200, &json(&bad)), "{pid}");
        }
        let mut negative = serde_json::to_value(&console).unwrap();
        negative["pid"] = (-5).into();
        assert!(unknown(200, negative.to_string().as_bytes()));
    }

    #[test]
    fn a_copy_must_answer_for_the_address_it_was_asked_at() {
        let own = 99;
        // A program on 8080 naming a copy that listens elsewhere: that copy's
        // stop token and process must never be reached through this answer.
        for address in ["127.0.0.1:9090", "127.0.0.2:8080", "[::1]:8080", "nonsense"] {
            for run in [
                RunKind::Console,
                RunKind::Service {
                    name: "VMQ-MVP".into(),
                },
            ] {
                let elsewhere = InstanceInfo {
                    address: address.into(),
                    ..info(run)
                };
                assert_eq!(
                    parse(200, &json(&elsewhere), asked(), own),
                    Occupant::Unknown,
                    "{address}"
                );
            }
        }
        // The same address written another way is the same address.
        let ipv6 = InstanceInfo {
            address: "[0:0:0:0:0:0:0:1]:8080".into(),
            ..info(RunKind::Console)
        };
        assert_eq!(
            parse(200, &json(&ipv6), "[::1]:8080".parse().unwrap(), own),
            Occupant::Ours(ipv6)
        );
        // `dx serve` answers on its proxy's port for the port it was given.
        let dev = InstanceInfo {
            address: "127.0.0.1:61234".into(),
            ..info(RunKind::Dev)
        };
        assert_eq!(parse(200, &json(&dev), asked(), own), Occupant::Ours(dev));
    }

    #[test]
    fn printed_texts_lose_control_characters() {
        let own = 99;
        let drawn = InstanceInfo {
            version: "0.9.0\r\nListening Exam Generator 9.9.9 is safe to stop\x1b[2K".into(),
            config_dir: "C:\\App\n\x1b]0;title\x07".into(),
            data_dir: "C:\\App\\data\t\u{9b}31m".into(),
            ..info(RunKind::Console)
        };
        let Occupant::Ours(shown) = parse(200, &json(&drawn), asked(), own) else {
            panic!("a copy with odd texts is still a copy");
        };
        assert_eq!(
            shown.version,
            "0.9.0Listening Exam Generator 9.9.9 is safe to stop[2K"
        );
        assert_eq!(shown.config_dir, "C:\\App]0;title");
        assert_eq!(shown.data_dir, "C:\\App\\data31m");
        assert_eq!(shown.pid, drawn.pid);
        assert_eq!(shown.run, drawn.run);
        // A service name `--service` would refuse is not a copy's.
        for name in ["VMQ\nMVP", "two words", "", "x'; Stop-Computer; '"] {
            let service = info(RunKind::Service { name: name.into() });
            assert_eq!(
                parse(200, &json(&service), asked(), own),
                Occupant::Unknown,
                "{name:?}"
            );
        }
        // A dev server must still name an address that can be printed.
        let dev = InstanceInfo {
            address: "127.0.0.1:8080\n".into(),
            ..info(RunKind::Dev)
        };
        assert_eq!(parse(200, &json(&dev), asked(), own), Occupant::Unknown);
    }

    #[tokio::test]
    async fn a_silent_or_foreign_port_is_unknown() {
        // Listens but never answers: the probe gives up.
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let started = std::time::Instant::now();
        assert_eq!(probe(silent.local_addr().unwrap()).await, Occupant::Unknown);
        assert!(started.elapsed() < Duration::from_secs(4));
        // Never probed: /instance answers only this computer.
        assert_eq!(
            probe("192.0.2.1:8080".parse().unwrap()).await,
            Occupant::Unknown
        );
    }

    #[tokio::test]
    async fn a_running_copy_is_recognised() {
        use dioxus::server::axum::{Router, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        // Another process's id, whatever this test process got.
        let answer = InstanceInfo {
            pid: std::process::id() + 1,
            address: address.to_string(),
            ..info(RunKind::Console)
        };
        let body = String::from_utf8(json(&answer)).unwrap();
        let router = Router::new().route(INFO_ROUTE, get(move || async move { body }));
        let server = tokio::spawn(async move {
            dioxus::server::axum::serve(listener, router).await.unwrap();
        });
        assert_eq!(probe(address).await, Occupant::Ours(answer));
        server.abort();
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        use dioxus::server::axum::{Router, response::Redirect, routing::get};
        // The copy the redirect points at would be recognised if asked directly.
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_address = target.local_addr().unwrap();
        let answer = InstanceInfo {
            pid: std::process::id() + 1,
            address: target_address.to_string(),
            ..info(RunKind::Console)
        };
        let body = String::from_utf8(json(&answer)).unwrap();
        let target_router = Router::new().route(INFO_ROUTE, get(move || async move { body }));
        let target_server = tokio::spawn(async move {
            dioxus::server::axum::serve(target, target_router)
                .await
                .unwrap();
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let location = format!("http://{target_address}{INFO_ROUTE}");
        let router = Router::new().route(
            INFO_ROUTE,
            get(move || async move { Redirect::temporary(&location) }),
        );
        let server = tokio::spawn(async move {
            dioxus::server::axum::serve(listener, router).await.unwrap();
        });
        assert_eq!(probe(target_address).await, Occupant::Ours(answer));
        assert_eq!(probe(address).await, Occupant::Unknown);
        server.abort();
        target_server.abort();
    }
}
