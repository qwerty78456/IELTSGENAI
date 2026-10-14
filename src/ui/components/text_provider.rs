//! The text-provider choice both pages show: whose model writes a run's
//! topics, scripts, questions and download-name summary. Recordings are
//! always Gemini's.

use dioxus::prelude::*;

use crate::application::settings::{KeyBoxPlace, key_box_place};
use crate::domain::TextProvider;
use crate::ui::components::key_setup::{KeyBox, KeyStatusCtx};

/// What an option reads: the provider, and whether it is the server's
/// `TEXT_PROVIDER` (unknown until the key statuses are here).
fn option_text(provider: TextProvider, text_default: Option<TextProvider>) -> String {
    if text_default == Some(provider) {
        format!("{} (server default)", provider.label())
    } else {
        provider.label().to_string()
    }
}

/// "Text written by:", one option per provider. `chosen` is the page's
/// choice; `None` follows the server's default, which the key statuses name,
/// so the choice waits for them. When the provider in effect is not the
/// default and its key is missing, its key box sits right under the choice
/// (`key_box_place`); a key the server needs anyway is asked for at the top.
#[component]
pub fn TextProviderPicker(
    chosen: Option<TextProvider>,
    disabled: bool,
    onchange: EventHandler<TextProvider>,
) -> Element {
    let KeyStatusCtx(statuses) = use_context::<KeyStatusCtx>();
    let current = statuses();
    let text_default = current.map(|s| s.text_default);
    let resolved = match text_default {
        Some(default) => TextProvider::resolve(chosen, default),
        None => chosen.unwrap_or(TextProvider::ALL[0]),
    };
    let key_beside = current.is_some_and(|s| {
        key_box_place(resolved, s.of(resolved), s.text_default, true) == KeyBoxPlace::BesideChoice
    });
    rsx! {
        div { class: "config-group",
            label { class: "input-label", "Text written by:" }
            div { class: "select-wrapper",
                select {
                    class: "section-select",
                    disabled: disabled || text_default.is_none(),
                    onchange: move |evt| {
                        if let Some(provider) = TextProvider::from_key(&evt.value()) {
                            onchange.call(provider);
                        }
                    },
                    for provider in TextProvider::ALL {
                        option {
                            value: "{provider.key()}",
                            selected: provider == resolved,
                            "{option_text(provider, text_default)}"
                        }
                    }
                }
            }
            p { class: "muted", "Recordings always use Gemini voices." }
        }
        if key_beside {
            KeyBox { provider: resolved }
        }
    }
}
