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

use ui::views::{Home, Navbar};

/// Internal routes of the app.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {
    #[cfg(feature = "server")]
    infrastructure::bootstrap();

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
