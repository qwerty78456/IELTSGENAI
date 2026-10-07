//! The release server's listeners: the main address and, with `PUBLIC_PORT`,
//! the published port the Cloudflare Tunnel connects to. Both serve one router
//! (built once) and stop on one process-wide signal.

use std::{io, net::SocketAddr, sync::LazyLock, time::Duration};

use dioxus::server::axum::{self, Extension, Router};
use tokio::{net::TcpListener, sync::watch};

use super::ingress::PublishedListener;

/// How long open connections (a long WAV download, a slow poll) may keep the
/// server up after the stop signal; then they are closed.
pub const DRAIN_DEADLINE: Duration = Duration::from_secs(5);

/// Which of the server's addresses a listener is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    /// `IP` and `PORT`.
    Main,
    /// `IP` and `PUBLIC_PORT`.
    Published,
}

/// A listener that could not be bound, kept typed so a caller can tell an
/// occupied port from other failures.
#[derive(Debug)]
pub struct BindError {
    pub listener: Listener,
    pub address: SocketAddr,
    pub error: io::Error,
}

impl BindError {
    /// Another program already listens on the port.
    pub fn in_use(&self) -> bool {
        self.error.kind() == io::ErrorKind::AddrInUse
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { address, error, .. } = self;
        match self.listener {
            Listener::Main => write!(
                f,
                "Cannot listen on {address}: {error}. Check IP/PORT or close the other application."
            ),
            Listener::Published => write!(
                f,
                "Cannot listen on {address} (PUBLIC_PORT): {error}. Check PUBLIC_PORT or close the other application."
            ),
        }
    }
}

/// Binds one listener.
pub async fn bind(listener: Listener, address: SocketAddr) -> Result<TcpListener, BindError> {
    TcpListener::bind(address).await.map_err(|error| BindError {
        listener,
        address,
        error,
    })
}

/// The URL a browser on this computer uses for `address`: an unspecified IP
/// (`0.0.0.0`, `::`) becomes loopback, and an IPv6 address is bracketed.
pub fn browser_url(address: SocketAddr) -> String {
    let ip = match address.ip() {
        ip if !ip.is_unspecified() => ip,
        std::net::IpAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
        std::net::IpAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
    };
    format!("http://{}", SocketAddr::new(ip, address.port()))
}

/// The process-wide stop request: `true` once something asked the server to stop.
static SHUTDOWN: LazyLock<watch::Sender<bool>> = LazyLock::new(|| watch::channel(false).0);

/// Asks every listener to stop: no new connections, and open ones get
/// `DRAIN_DEADLINE` to finish. A request made before serving starts is kept.
pub fn request_shutdown() {
    SHUTDOWN.send_replace(true);
}

/// Follows the process-wide stop request, for `serve`.
pub fn shutdown_requests() -> watch::Receiver<bool> {
    SHUTDOWN.subscribe()
}

/// Turns Ctrl+C, Ctrl+Break (Windows) and SIGTERM (Unix: `docker stop`,
/// systemd) into a stop request. Call inside the runtime.
pub fn stop_on_signals() {
    tokio::spawn(async {
        os_signal().await;
        tracing::info!("Stop signal received");
        request_shutdown();
    });
}

async fn os_signal() {
    let interrupt = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(windows)]
    let other = async {
        match tokio::signal::windows::ctrl_break() {
            Ok(mut ctrl_break) => {
                ctrl_break.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(unix)]
    let other = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                terminate.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(any(windows, unix)))]
    let other = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = other => {}
    }
}

/// Serves `router` on `main` and, when given, on `published`, until `shutdown`
/// turns true; then waits at most `DRAIN_DEADLINE` for open connections.
/// Requests on `published` carry `PublishedListener`, layered after every
/// route so pages, server functions, assets and plain routes all see it.
pub async fn serve(
    router: Router,
    main: TcpListener,
    published: Option<TcpListener>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    serve_draining(router, main, published, shutdown, DRAIN_DEADLINE).await
}

async fn serve_draining(
    router: Router,
    main: TcpListener,
    published: Option<TcpListener>,
    shutdown: watch::Receiver<bool>,
    drain: Duration,
) -> Result<(), String> {
    let published_router = router.clone().layer(Extension(PublishedListener));
    let main_server = async {
        axum::serve(main, router)
            .with_graceful_shutdown(stopped(shutdown.clone()))
            .await
    };
    let published_server = async {
        match published {
            Some(listener) => {
                axum::serve(listener, published_router)
                    .with_graceful_shutdown(stopped(shutdown.clone()))
                    .await
            }
            None => Ok(()),
        }
    };
    let servers = async { tokio::try_join!(main_server, published_server).map(|_| ()) };
    let deadline = async {
        stopped(shutdown.clone()).await;
        tokio::time::sleep(drain).await;
    };
    tokio::select! {
        result = servers => result.map_err(|e| format!("Server stopped unexpectedly: {e}")),
        () = deadline => {
            tracing::warn!(
                "Connections still open {} s after the stop signal were closed",
                drain.as_secs()
            );
            Ok(())
        }
    }
}

/// Resolves once a stop is requested (or nothing can request one any more).
async fn stopped(mut shutdown: watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|stop| *stop).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::server::axum::{extract::Request, routing::get};

    #[test]
    fn browser_urls_name_a_reachable_address() {
        let url = |address: &str| browser_url(address.parse().unwrap());
        assert_eq!(url("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(url("0.0.0.0:8080"), "http://127.0.0.1:8080");
        assert_eq!(url("[::]:8081"), "http://[::1]:8081");
        assert_eq!(url("[::1]:8081"), "http://[::1]:8081");
        assert_eq!(url("192.168.1.10:80"), "http://192.168.1.10:80");
    }

    #[test]
    fn bind_errors_name_the_setting_to_check() {
        let error = |listener| BindError {
            listener,
            address: "127.0.0.1:8081".parse().unwrap(),
            error: io::Error::from(io::ErrorKind::AddrInUse),
        };
        let main = error(Listener::Main);
        assert!(main.in_use());
        let text = main.to_string();
        assert!(
            text.starts_with("Cannot listen on 127.0.0.1:8081: "),
            "{text}"
        );
        assert!(text.ends_with(". Check IP/PORT or close the other application."));
        let text = error(Listener::Published).to_string();
        assert!(
            text.starts_with("Cannot listen on 127.0.0.1:8081 (PUBLIC_PORT): "),
            "{text}"
        );
        assert!(text.ends_with(". Check PUBLIC_PORT or close the other application."));
        let denied = BindError {
            error: io::Error::from(io::ErrorKind::PermissionDenied),
            ..error(Listener::Main)
        };
        assert!(!denied.in_use());
    }

    #[tokio::test]
    async fn an_occupied_port_is_reported_as_in_use() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = taken.local_addr().unwrap();
        let error = bind(Listener::Published, address).await.unwrap_err();
        assert!(error.in_use(), "{error}");
        assert_eq!(error.address, address);
    }

    async fn published(request: Request) -> &'static str {
        if request.extensions().get::<PublishedListener>().is_some() {
            "published"
        } else {
            "main"
        }
    }

    async fn fetch(address: SocketAddr, path: &str) -> String {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}{path}"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn one_router_two_listeners_one_stop() {
        // A route and the fallback: the marker must reach both.
        let router = Router::new()
            .route("/route", get(published))
            .fallback(published);
        let main = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let public = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (main_address, public_address) =
            (main.local_addr().unwrap(), public.local_addr().unwrap());
        let (stop, shutdown) = watch::channel(false);
        let server = tokio::spawn(serve(router, main, Some(public), shutdown));
        for path in ["/route", "/page"] {
            assert_eq!(fetch(main_address, path).await, "main");
            assert_eq!(fetch(public_address, path).await, "published");
        }
        stop.send_replace(true);
        let stopped = tokio::time::timeout(DRAIN_DEADLINE * 2, server).await;
        assert_eq!(stopped.unwrap().unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn a_hanging_download_does_not_hold_the_stop() {
        let router = Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                "late"
            }),
        );
        let main = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = main.local_addr().unwrap();
        let (stop, shutdown) = watch::channel(false);
        let drain = Duration::from_millis(300);
        let server = tokio::spawn(serve_draining(router, main, None, shutdown, drain));
        let download = tokio::spawn(fetch(address, "/slow"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let asked = std::time::Instant::now();
        stop.send_replace(true);
        let stopped = tokio::time::timeout(Duration::from_secs(5), server).await;
        assert_eq!(stopped.unwrap().unwrap(), Ok(()));
        assert!(asked.elapsed() >= drain, "{:?}", asked.elapsed());
        download.abort();
    }

    #[tokio::test]
    async fn a_stop_requested_before_serving_is_not_lost() {
        let (stop, shutdown) = watch::channel(false);
        stop.send_replace(true);
        let main = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let served = tokio::time::timeout(
            Duration::from_secs(2),
            serve(Router::new(), main, None, shutdown),
        )
        .await;
        assert_eq!(served.unwrap(), Ok(()));
    }
}
