use crate::Route;
use dioxus::prelude::*;

use super::exam::{ExamState, SaveStatusPanel};
use super::home::HomeState;
use crate::application::voices::voice_catalogue;
use crate::ui::components::key_setup::{KeySetup, KeyStatusCtx};
use crate::ui::components::voices::VoiceCatalogueCtx;
use crate::ui::save_queue::SaveQueue;

const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");

/// Rendered on every page; hosts the router outlet. Both pages' states live
/// here (the part page's `HomeState`, the exam draft with its save queue) so
/// that moving between pages drops neither a running part nor a running
/// exam, and so do the voice catalogue, loaded once for both pages, and the
/// API key statuses, kept up to date by `KeySetup`.
#[component]
pub fn Navbar() -> Element {
    let saves = use_context_provider(|| Signal::new(SaveQueue::default()));
    use_context_provider(|| Signal::new(ExamState::with_queue(saves)));
    use_context_provider(|| Signal::new(HomeState::default()));
    let catalogue = use_resource(|| async { voice_catalogue().await.map_err(|e| e.to_string()) });
    use_context_provider(|| VoiceCatalogueCtx(catalogue));
    use_context_provider(|| KeyStatusCtx(Signal::new(None)));

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
        SaveStatusPanel {}

        Outlet::<Route> {}
    }
}
