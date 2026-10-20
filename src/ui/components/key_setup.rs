use dioxus::prelude::*;

use crate::application::settings::{
    KeyBoxPlace, KeySource, KeyStatuses, api_key_status, key_box_place, set_api_key,
};
use crate::domain::TextProvider;
use crate::ui::jobs::sleep_ms;

const KEY_SETUP_CSS: Asset = asset!("/assets/styling/key_setup.css");
/// How often the key status is asked again, so a key its company rejects
/// during any step (a script, questions, a recording) brings its box up. The
/// server answers from memory; nothing is sent to Google or Anthropic.
const KEY_STATUS_POLL_MS: u32 = 5_000;
/// Failed polls in a row before the page says why.
const KEY_STATUS_FAILURES_SHOWN: u32 = 3;

/// Both providers' key statuses, provided by the `Navbar` layout; `None`
/// until the first answer. Written by `KeySetup`'s poll and by a `KeyBox`
/// whose key was accepted.
#[derive(Clone, Copy)]
pub struct KeyStatusCtx(pub Signal<Option<KeyStatuses>>);

/// Shown on every page: keeps `KeyStatusCtx` up to date, and puts the key box
/// of each provider whose key the server needs whatever a page chooses (see
/// `key_box_place`) above the page while that key is missing or rejected.
#[component]
pub fn KeySetup() -> Element {
    let KeyStatusCtx(mut statuses) = use_context::<KeyStatusCtx>();
    // Why the server keeps refusing this page (opened by another address
    // than its own, or the server is gone): every other step would fail the
    // same way. Shown after a few failures in a row, so a restart passes.
    let mut refused = use_signal(|| None::<String>);

    use_future(move || async move {
        let mut failures = 0;
        loop {
            match api_key_status().await {
                Ok(current) => {
                    failures = 0;
                    let changed = *statuses.peek() != Some(current);
                    if changed {
                        statuses.set(Some(current));
                    }
                    if refused.peek().is_some() {
                        refused.set(None);
                    }
                }
                Err(error) => {
                    failures += 1;
                    let message = Some(error.to_string());
                    if failures >= KEY_STATUS_FAILURES_SHOWN && *refused.peek() != message {
                        refused.set(message);
                    }
                }
            }
            sleep_ms(KEY_STATUS_POLL_MS).await;
        }
    });

    if let Some(message) = refused() {
        return rsx! {
            section { class: "key-setup", role: "alert",
                p { "{message}" }
            }
        };
    }
    let Some(current) = statuses() else {
        return rsx! {};
    };
    let at_top = |provider: TextProvider| {
        key_box_place(provider, current.of(provider), current.text_default, false)
            == KeyBoxPlace::Top
    };
    // One `if` per provider, so a box that goes away never hands its typed
    // key or error to the other.
    rsx! {
        if at_top(TextProvider::Gemini) {
            KeyBox { provider: TextProvider::Gemini }
        }
        if at_top(TextProvider::Anthropic) {
            KeyBox { provider: TextProvider::Anthropic }
        }
    }
}

/// What a key box says that differs by provider.
struct BoxTexts {
    /// The API the key may not be allowed to use.
    api: &'static str,
    /// What anyone who can open the page could do with the key.
    spend: &'static str,
    /// How to limit the damage of a leaked key.
    advice: &'static str,
}

fn box_texts(provider: TextProvider) -> BoxTexts {
    match provider {
        TextProvider::Gemini => BoxTexts {
            api: "the Gemini API",
            spend: "spend your Gemini credit",
            advice: "Use a key restricted to the Generative Language API, and revoke it in Google AI Studio if you suspect it leaked.",
        },
        TextProvider::Anthropic => BoxTexts {
            api: "the Claude API",
            spend: "spend on your Anthropic account",
            advice: "Set a spend limit for the key's workspace in the Claude Console, and delete the key there if you suspect it leaked. The free check proves the key works, not that the account has credit.",
        },
    }
}

/// `provider`'s key box, while its key is missing or rejected: explains the
/// risk, then lets the teacher paste a key (only in a browser on the
/// server's own computer). Where it goes is the caller's choice
/// (`key_box_place`).
#[component]
pub fn KeyBox(provider: TextProvider) -> Element {
    let KeyStatusCtx(mut statuses) = use_context::<KeyStatusCtx>();
    let mut key = use_signal(String::new);
    let mut remember = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let Some(current) = statuses().map(|all| all.of(provider)) else {
        return rsx! {};
    };
    let missing = current.source == KeySource::Missing;
    if !missing && !current.rejected {
        return rsx! {};
    }
    let key_env = provider.key_env();
    let key_name = provider.key_name();
    let company = provider.company();
    let BoxTexts { api, spend, advice } = box_texts(provider);
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
                h2 { "{key_name} needed" }
                p {
                    "No key was found in the "
                    code { "{key_env}" }
                    " environment variable (process or Windows) or in "
                    code { ".env" }
                    ". Setting the environment variable and restarting is the safe way."
                }
            } else {
                h2 { "{company} rejected the {key_name}" }
                p {
                    "The key from {place} does not work: it may be mistyped, revoked, or not allowed to use {api}. "
                    if operator_key {
                        "Fix "
                        code { "{key_env}" }
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
                    ": anyone who can open this page can {spend}. Only do this in a browser on the server's own computer. Prefer setting the "
                    code { "{key_env}" }
                    " environment variable. {advice}"
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
                            let answer = set_api_key(provider, typed, remember).await;
                            // Before the shared statuses: an accepted key
                            // takes this box away.
                            busy.set(false);
                            match answer {
                                Ok(current) => statuses.set(Some(current)),
                                Err(e) => error.set(Some(e.to_string())),
                            }
                        });
                    },
                    input {
                        class: "form-input",
                        r#type: "password",
                        autocomplete: "off",
                        spellcheck: "false",
                        placeholder: "Paste your {key_name}",
                        aria_label: "{key_name}",
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
                            code { "{key_env}" }
                            " in {priority_place} takes priority over "
                            code { ".env" }
                            "."
                        }
                    }
                    button {
                        class: "download-button info",
                        r#type: "submit",
                        disabled: busy() || key.read().trim().is_empty(),
                        if busy() { "Checking with {company}…" } else { "Use this key" }
                    }
                }
            } else {
                p { class: "muted",
                    "A key can be entered here only in a browser on the server's own computer, and not at all while the server listens on a network address (IP=0.0.0.0 or a LAN address, as in Docker). Ask whoever runs the server to set "
                    code { "{key_env}" }
                    " and restart it."
                }
            }
            if let Some(message) = error() {
                div { class: "error-box", "{message}" }
            }
        }
    }
}
