use dioxus::prelude::*;

/// Plays a recording the server streams at `src` and offers it as a download.
/// The bytes never pass through the browser's memory as a blob; the `<audio>`
/// element seeks with `Range` requests and the link downloads directly.
#[component]
pub fn AudioPlayerSection(src: String, file_name: String, duration_ms: Option<u32>) -> Element {
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
                a {
                    class: "download-button primary small",
                    href: "{src}",
                    download: "{file_name}",
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

/// Triggers a browser download of in-memory bytes.
#[cfg(target_arch = "wasm32")]
pub fn download_bytes(data: &[u8], mime: &str, file_name: &str) {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url, window};

    let Some(document) = window().and_then(|w| w.document()) else {
        return;
    };
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
    if let Ok(anchor) = document.create_element("a") {
        let anchor: HtmlAnchorElement = anchor.unchecked_into();
        anchor.set_href(&url);
        anchor.set_download(file_name);
        anchor.click();
        let _ = Url::revoke_object_url(&url);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn download_bytes(_data: &[u8], _mime: &str, _file_name: &str) {}

/// Triggers a browser download of a UTF-8 text file.
pub fn download_text(content: &str, file_name: &str) {
    download_bytes(content.as_bytes(), "text/plain;charset=utf-8", file_name);
}
