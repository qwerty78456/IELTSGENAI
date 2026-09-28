use dioxus::prelude::*;

use crate::application::settings::{KeySource, KeyStatus, api_key_status, set_api_key};

const KEY_SETUP_CSS: Asset = asset!("/assets/styling/key_setup.css");

/// Shown on every page while the server has no Gemini API key: explains the
/// risk, then lets the teacher paste a key (on a local server only).
#[component]
pub fn KeySetup() -> Element {
    let mut status = use_signal(|| None::<KeyStatus>);
    let mut key = use_signal(String::new);
    let mut remember = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    use_future(move || async move {
        if let Ok(current) = api_key_status().await {
            status.set(Some(current));
        }
    });

    let Some(current) = status() else {
        return rsx! {};
    };
    if current.source != KeySource::Missing {
        return rsx! {};
    }
    rsx! {
        document::Link { rel: "stylesheet", href: KEY_SETUP_CSS }
        section { class: "key-setup",
            h2 { "Gemini API key needed" }
            p {
                "No key was found in the "
                code { "GEMINI_API_KEY" }
                " environment variable (process or Windows) or in "
                code { ".env" }
                ". Setting the environment variable and restarting is the safe way."
            }
            div { class: "key-warning", role: "alert",
                p { class: "key-warning-title", "⚠ Security — read before pasting your key." }
                p {
                    "The key is sent from this browser to the server over plain HTTP and kept in the server's memory; with \"Remember\" it is written "
                    strong { "unencrypted" }
                    " to "
                    code { ".env" }
                    ". This app has "
                    strong { "no login" }
                    ": anyone who can open this page can spend your Gemini credit. Only do this on your own computer with the app bound to 127.0.0.1. Prefer setting the "
                    code { "GEMINI_API_KEY" }
                    " environment variable. Use a key restricted to the Generative Language API, and revoke it in Google AI Studio if you suspect it leaked."
                }
            }
            if current.can_enter {
                form {
                    class: "key-form",
                    autocomplete: "off",
                    onsubmit: move |event: FormEvent| {
                        event.prevent_default();
                        // Clear the field first so the key does not stay in the page.
                        let typed = key();
                        key.set(String::new());
                        let remember = remember();
                        busy.set(true);
                        error.set(None);
                        spawn(async move {
                            match set_api_key(typed, remember).await {
                                Ok(current) => status.set(Some(current)),
                                Err(e) => error.set(Some(e.to_string())),
                            }
                            busy.set(false);
                        });
                    },
                    input {
                        class: "form-input",
                        r#type: "password",
                        autocomplete: "off",
                        spellcheck: "false",
                        placeholder: "Paste your Gemini API key",
                        aria_label: "Gemini API key",
                        value: "{key}",
                        disabled: busy(),
                        oninput: move |event| key.set(event.value()),
                    }
                    label { class: "key-remember",
                        input {
                            r#type: "checkbox",
                            checked: remember(),
                            disabled: busy(),
                            onchange: move |event| remember.set(event.checked()),
                        }
                        " Remember on this computer (writes the key unencrypted to .env)"
                    }
                    button {
                        class: "download-button info",
                        r#type: "submit",
                        disabled: busy() || key.read().trim().is_empty(),
                        if busy() { "Checking with Google…" } else { "Use this key" }
                    }
                }
            } else {
                p { class: "muted",
                    "This server is reachable from other computers, so it does not accept a key from the browser. Set "
                    code { "GEMINI_API_KEY" }
                    " on the server and restart."
                }
            }
            if let Some(message) = error() {
                div { class: "error-box", "{message}" }
            }
        }
    }
}
