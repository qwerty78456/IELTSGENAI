use dioxus::prelude::*;

#[component]
pub fn LoadingPopup(message: String, submessage: String, oncancel: EventHandler<()>) -> Element {
    rsx! {
        div { class: "loading-popup-container",
            div { class: "loading-popup-content",
                div { class: "loading-popup-header",
                    div { class: "loading-popup-header-content",
                        div { class: "loading-popup-spinner" }
                        div { class: "loading-popup-text-content",
                            p { class: "loading-popup-text", "{message}" }
                            p { class: "loading-popup-subtext", "{submessage}" }
                        }
                    }
                    button {
                        class: "loading-popup-close",
                        onclick: move |_| oncancel.call(()),
                        title: "Cancel generation",
                        "✕"
                    }
                }
            }
        }
    }
}
