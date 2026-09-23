//! Portable lifecycle, with explicit errors before opening a browser.
use super::{
    config::{StartupOptions, config},
    jobs,
};
use dioxus::prelude::*;

fn router(app: fn() -> Element) -> dioxus::server::axum::Router {
    dioxus::server::router(app).route(
        crate::application::audio::AUDIO_ROUTE,
        dioxus::server::axum::routing::get(jobs::serve_audio),
    )
}

pub fn run(app: fn() -> Element) -> Result<(), String> {
    let options = StartupOptions::parse(std::env::args_os().skip(1))?;
    let _log_guard = super::bootstrap(&options)?;
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
    if !options.portable && cfg!(debug_assertions) {
        dioxus::serve(move || async move {
            if let Err(error) = jobs::JobStore::initialize().await {
                eprintln!("ERROR: {error}");
                std::process::exit(1);
            }
            jobs::ensure_cleanup_running();
            Ok(router(app))
        });
    }
    let runtime =
        tokio::runtime::Runtime::new().map_err(|e| format!("Cannot start runtime: {e}"))?;
    runtime.block_on(async {
        jobs::JobStore::initialize().await?;
        let addr = config().address;
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
            format!("Cannot listen on {addr}: {e}. Check IP/PORT or close the other application.")
        })?;
        let app_router = router(app);
        jobs::ensure_cleanup_running();
        let browser_ip = if addr.ip().is_unspecified() {
            if addr.is_ipv6() {
                std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
            } else {
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            }
        } else {
            addr.ip()
        };
        let url = format!(
            "http://{}",
            std::net::SocketAddr::new(browser_ip, addr.port())
        );
        println!("Listening Exam Generator: {url}\nPress Ctrl+C to stop.");
        if options.portable && !options.no_open {
            let browser_url = url.clone();
            // A desktop opener may wait for its browser. Do not make runtime shutdown
            // wait for it, and do not pass AppImage libraries into unrelated programs.
            std::thread::spawn(move || {
                let opened = open::commands(&browser_url).into_iter().any(|mut command| {
                    command
                        .env_remove("LD_LIBRARY_PATH")
                        .env_remove("DIOXUS_PUBLIC_PATH")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .is_ok_and(|status| status.success())
                });
                if !opened {
                    eprintln!("Could not open the browser. Open {browser_url} manually.");
                }
            });
        }
        dioxus::server::axum::serve(listener, app_router)
            .with_graceful_shutdown(shutdown())
            .await
            .map_err(|e| format!("Server stopped unexpectedly: {e}"))?;
        println!("Server stopped cleanly.");
        Ok(())
    })
}

async fn shutdown() {
    #[cfg(windows)]
    if let Ok(mut interrupt) = tokio::signal::windows::ctrl_break() {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = interrupt.recv() => {}
        }
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}
