use crate::Route;
use dioxus::prelude::*;

use super::exam::ExamState;
use crate::application::voices::voice_catalogue;
use crate::ui::components::key_setup::KeySetup;
use crate::ui::components::voices::VoiceCatalogueCtx;

const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");

/// Rendered on every page; hosts the router outlet. The exam draft lives here
/// so that moving between pages does not drop a running exam, and so does the
/// voice catalogue, loaded once for both pages.
#[component]
pub fn Navbar() -> Element {
    use_context_provider(|| Signal::new(ExamState::default()));
    let catalogue = use_resource(|| async { voice_catalogue().await.map_err(|e| e.to_string()) });
    use_context_provider(|| VoiceCatalogueCtx(catalogue));

    rsx! {
        document::Link { rel: "stylesheet", href: NAVBAR_CSS }

        div {
            id: "navbar",
            Link {
                to: Route::Home {},
                "Listening Exam Generator"
            }
            Link {
                to: Route::Home {},
                "One part"
            }
            Link {
                to: Route::ExamView {},
                "Whole exam"
            }
        }

        KeySetup {}

        Outlet::<Route> {}
    }
}
