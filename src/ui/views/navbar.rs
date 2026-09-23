use crate::Route;
use dioxus::prelude::*;

use super::exam::ExamState;

const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");

/// Rendered on every page; hosts the router outlet. The exam draft lives here
/// so that moving between pages does not drop a running exam.
#[component]
pub fn Navbar() -> Element {
    use_context_provider(|| Signal::new(ExamState::default()));

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

        Outlet::<Route> {}
    }
}
