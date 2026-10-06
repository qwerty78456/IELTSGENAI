use dioxus::prelude::*;

use crate::ui::prefs;

/// How often the switch looks at the stored choice, so a change made in
/// another tab shows here too.
#[cfg(target_arch = "wasm32")]
const RECHECK_MS: u32 = 2_000;

/// The switch for downloading the DOCX and the WAV as soon as each is ready.
/// The choice lives in this browser (`ui::prefs`), shared by both pages and
/// every tab. Pipelines read it when a file is ready, so it is never disabled
/// while one runs: switching it on mid-run still downloads that run's files.
#[component]
pub fn AutoDownloadToggle() -> Element {
    let mut on = use_signal(|| false);
    // Read after mount (the server render has no browser storage, and the
    // first browser render must match it), then again every few seconds.
    use_future(move || async move {
        #[cfg(target_arch = "wasm32")]
        loop {
            let stored = prefs::auto_download();
            if *on.peek() != stored {
                on.set(stored);
            }
            crate::ui::jobs::sleep_ms(RECHECK_MS).await;
        }
    });

    rsx! {
        label { class: "expressive-toggle",
            input {
                r#type: "checkbox",
                checked: on(),
                onchange: move |evt| {
                    let checked = evt.checked();
                    on.set(checked);
                    prefs::set_auto_download(checked);
                },
            }
            " Download the DOCX and WAV automatically"
        }
        p { class: "muted",
            "Each file is saved as soon as it is ready: the DOCX when the questions are written, the WAV when the recording is. Your browser may ask once to allow several downloads."
        }
    }
}
