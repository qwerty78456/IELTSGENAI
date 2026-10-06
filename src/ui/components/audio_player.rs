use dioxus::prelude::*;

/// Plays a recording the server streams at `src` and offers it as a download.
/// The bytes never pass through the browser's memory as a blob: the `<audio>`
/// element seeks with `Range` requests, and the button calls `ondownload`,
/// which names the file (`ui::naming`) and downloads `src` directly.
#[component]
pub fn AudioPlayerSection(
    src: String,
    duration_ms: Option<u32>,
    ondownload: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "audio-player-section",
            div { class: "audio-player",
                audio {
                    controls: true,
                    preload: "metadata",
                    src: "{src}",
                }
            }
            div { class: "audio-controls",
                if let Some(ms) = duration_ms {
                    span { class: "audio-duration", "Length {format_duration(ms)}" }
                }
                button {
                    class: "download-button primary small",
                    onclick: move |_| ondownload.call(()),
                    "Download audio (WAV)"
                }
            }
        }
    }
}

/// m:ss for a duration in milliseconds.
pub fn format_duration(ms: u32) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

/// How long a downloaded blob's URL stays valid: long enough for any browser
/// to start saving it, short enough not to keep documents in memory.
#[cfg(target_arch = "wasm32")]
const BLOB_URL_LIFETIME_MS: u32 = 60_000;

/// Clicks a temporary `<a download>` for `href`, attached to the page while
/// it is clicked (older Firefox ignores a detached one).
#[cfg(target_arch = "wasm32")]
fn click_download(href: &str, file_name: &str) {
    use wasm_bindgen::JsCast;
    use web_sys::{HtmlAnchorElement, window};

    let Some(document) = window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(anchor) = document.create_element("a") else {
        return;
    };
    let anchor: HtmlAnchorElement = anchor.unchecked_into();
    anchor.set_href(href);
    anchor.set_download(file_name);
    if let Some(body) = document.body() {
        let _ = body.append_child(&anchor);
    }
    anchor.click();
    anchor.remove();
}

/// Triggers a browser download of in-memory bytes.
#[cfg(target_arch = "wasm32")]
pub fn download_bytes(data: &[u8], mime: &str, file_name: &str) {
    use web_sys::{Blob, BlobPropertyBag, Url};

    let uint8_array = js_sys::Uint8Array::new_with_length(data.len() as u32);
    uint8_array.copy_from(data);
    let array = js_sys::Array::new();
    array.push(&uint8_array);
    let blob_options = BlobPropertyBag::new();
    blob_options.set_type(mime);
    let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&array, &blob_options) else {
        return;
    };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else {
        return;
    };
    click_download(&url, file_name);
    // Revoked later, not at once: a download started without a click (the
    // automatic ones) may still be reading the blob.
    gloo_timers::callback::Timeout::new(BLOB_URL_LIFETIME_MS, move || {
        let _ = Url::revoke_object_url(&url);
    })
    .forget();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn download_bytes(_data: &[u8], _mime: &str, _file_name: &str) {}

/// Triggers a browser download of a file the server serves (a recording),
/// saved under `file_name`. Same-origin, so the name is honoured.
#[cfg(target_arch = "wasm32")]
pub fn download_url(url: &str, file_name: &str) {
    click_download(url, file_name);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn download_url(_url: &str, _file_name: &str) {}

/// Triggers a browser download of a UTF-8 text file.
pub fn download_text(content: &str, file_name: &str) {
    download_bytes(content.as_bytes(), "text/plain;charset=utf-8", file_name);
}
