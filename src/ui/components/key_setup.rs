use dioxus::prelude::*;

use crate::application::settings::{KeySource, KeyStatus, api_key_status, set_api_key};
use crate::ui::jobs::sleep_ms;

const KEY_SETUP_CSS: Asset = asset!("/assets/styling/key_setup.css");
/// How often the key status is asked again, so a key Google rejects during
/// any step (a script, questions, a recording) brings this box up. The server
/// answers from memory; nothing is sent to Google.
const KEY_STATUS_POLL_MS: u32 = 5_000;

/// Shown on every page while the server has no Gemini API key or Google
/// rejected the one in use: explains the risk, then lets the teacher paste a
/// key (only in a browser on the server's own computer).
#[component]
pub fn KeySetup() -> Element {
    let mut status = use_signal(|| None::<KeyStatus>);
    let mut key = use_signal(String::new);
    let mut remember = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    use_future(move || async move {
        loop {
            if let Ok(current) = api_key_status().await
                && *status.peek() != Some(current)
            {
                status.set(Some(current));
            }
            sleep_ms(KEY_STATUS_POLL_MS).await;
        }
    });

    let Some(current) = status() else {
        return rsx! {};
    };
    let missing = current.source == KeySource::Missing;
    if !missing && !current.rejected {
        return rsx! {};
    }
    let operator_key = matches!(
        current.source,
        KeySource::Environment | KeySource::WindowsEnvironment | KeySource::DotEnv
    );
    let place = current.source.describe();
    // A browser key that was itself rejected sits over an environment key.
    let priority_place = if operator_key {
        place
    } else {
        "the environment"
    };
    rsx! {
        document::Link { rel: "stylesheet", href: KEY_SETUP_CSS }
        section { class: "key-setup",
            if missing {
                h2 { "Gemini API key needed" }
                p {
                    "No key was found in the "
                    code { "GEMINI_API_KEY" }
                    " environment variable (process or Windows) or in "
                    code { ".env" }
                    ". Setting the environment variable and restarting is the safe way."
                }
            } else {
                h2 { "Google rejected the Gemini API key" }
                p {
                    "The key from {place} does not work: it may be mistyped, revoked, or not allowed to use the Gemini API. "
                    if operator_key {
                        "Fix "
                        code { "GEMINI_API_KEY" }
                        " there and restart the app"
                        if current.can_enter {
                            ", or paste a working key below; it replaces the rejected key until the app restarts."
                        } else {
                            "."
                        }
                    } else if current.can_enter {
                        "Paste a working key below."
                    }
                }
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
                    ": anyone who can open this page can spend your Gemini credit. Only do this in a browser on the server's own computer. Prefer setting the "
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
                        let remember = remember() && current.can_remember;
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
                    if current.can_remember {
                        label { class: "key-remember",
                            input {
                                r#type: "checkbox",
                                checked: remember(),
                                disabled: busy(),
                                onchange: move |event| remember.set(event.checked()),
                            }
                            " Remember on this computer (writes the key unencrypted to .env)"
                        }
                    } else {
                        p { class: "muted",
                            "A key pasted here lasts until the app restarts: "
                            code { "GEMINI_API_KEY" }
                            " in {priority_place} takes priority over "
                            code { ".env" }
                            "."
                        }
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
                    "A key can be entered here only in a browser on the server's own computer, and not at all while the server listens on a network address (IP=0.0.0.0 or a LAN address, as in Docker). Ask whoever runs the server to set "
                    code { "GEMINI_API_KEY" }
                    " and restart it."
                }
            }
            if let Some(message) = error() {
                div { class: "error-box", "{message}" }
            }
        }
    }
}
