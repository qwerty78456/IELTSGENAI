use dioxus::prelude::*;

use views::{Home, Navbar};

/// Define a components module that contains all shared components for our app.
mod components;
/// Define a domain module that contains the domain model
mod domain;
/// Define a services module that contains business logic services
mod services;
/// Define a views module that contains the UI for all Layouts and Routes for our app.
mod views;

/// The Route enum is used to define the structure of internal routes in our app.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
}

// We can import assets in dioxus with the `asset!` macro. This macro takes a path to an asset relative to the crate root.
// The macro returns an `Asset` type that will display as the path to the asset in the browser or a local path in desktop bundles.
const FAVICON: Asset = asset!("/assets/favicon.ico");
// The asset macro also minifies some assets like CSS and JS to make bundled smaller
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {
    // Server-side initialization
    #[cfg(feature = "server")]
    {
        // Bind to all interfaces on port 80 (override with IP/PORT env vars if set)
        // SAFETY: Called in main() before any threads are spawned, so no data race.
        unsafe {
            if std::env::var("IP").is_err() {
                std::env::set_var("IP", "0.0.0.0");
            }
            if std::env::var("PORT").is_err() {
                std::env::set_var("PORT", "80");
            }
        }

        // Initialize rate limiters
        services::rate_limiter::init_rate_limiters();

        // Ensure audio storage directory exists
        if let Err(e) = services::audio_job_manager::ensure_audio_dir() {
            eprintln!("WARNING: {}", e);
            eprintln!("Audio generation may fail. Please create the directory manually.");
        }
    }

    // Launch the Dioxus app
    dioxus::launch(App);

    // Start background cleanup task (must be after launch initializes the Tokio runtime)
    // Note: In Dioxus fullstack, the server-side runtime is set up by launch().
    // The cleanup task is started within the audio_job_manager module's lazy init instead.
}

/// App is the main component of our app. Components are the building blocks of dioxus apps. Each component is a function
/// that takes some props and returns an Element. In this case, App takes no props because it is the root of our app.
///
/// Components should be annotated with `#[component]` to support props, better error messages, and autocomplete
#[component]
fn App() -> Element {
    // The `rsx!` macro lets us define HTML inside of rust. It expands to an Element with all of our HTML inside.
    rsx! {
        // In addition to element and text (which we will see later), rsx can contain other components. In this case,
        // we are using the `document::Link` component to add a link to our favicon and main CSS file into the head of our app.
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }


        // The router component renders the route enum we defined above. It will handle synchronization of the URL and render
        // the layouts and components for the active route.
        Router::<Route> {}
    }
}
