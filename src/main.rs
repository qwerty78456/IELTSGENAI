//! Listening exam generator: one Dioxus fullstack binary.
//!
//! Layers (see `docs/architecture.md`):
//! - `domain`         pure types and rules, shared by browser and server
//! - `application`    the `#[server]` functions the UI calls (use cases)
//! - `infrastructure` server-only adapters: Gemini, TTS, WAV, SQLite jobs
//! - `export`         pure renderers (Markdown paper, key, transcript)
//! - `ui`             Dioxus components and views

use dioxus::prelude::*;

mod application;
mod domain;
mod export;
#[cfg(feature = "server")]
mod infrastructure;
mod ui;

use ui::views::{ExamView, Home, Navbar};

/// Internal routes of the app.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[route("/exam")]
        ExamView {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {
    #[cfg(feature = "server")]
    {
        infrastructure::bootstrap();
        // The Dioxus router plus one plain axum route that streams finished
        // recordings. The closure runs inside the Tokio runtime, so the hourly
        // clean-up starts here rather than on the first request.
        dioxus::serve(|| async {
            infrastructure::jobs::ensure_cleanup_running();
            Ok(dioxus::server::router(App).route(
                application::audio::AUDIO_ROUTE,
                dioxus::server::axum::routing::get(infrastructure::jobs::serve_audio),
            ))
        });
    }

    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}
