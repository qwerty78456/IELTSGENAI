use dioxus::prelude::*;

/// Plays a WAV held in memory and offers it as a download.
#[component]
pub fn AudioPlayerSection(audio_data: Vec<u8>, file_name: String) -> Element {
    let mut is_audio_playing = use_signal(|| false);

    rsx! {
        div { class: "audio-player-section",
            div { class: "audio-player",
                audio {
                    id: "audio-element",
                    controls: true,
                    src: create_audio_url(&audio_data),
                    onplay: move |_| is_audio_playing.set(true),
                    onpause: move |_| is_audio_playing.set(false),
                    onended: move |_| is_audio_playing.set(false),
                }
            }
            div { class: "audio-controls",
                button {
                    class: "download-button primary small",
                    onclick: move |_| download_bytes(&audio_data, "audio/wav", &file_name),
                    "Download audio (WAV)"
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn create_audio_url(audio_data: &[u8]) -> String {
    use web_sys::{Blob, BlobPropertyBag, Url};

    let uint8_array = js_sys::Uint8Array::new_with_length(audio_data.len() as u32);
    uint8_array.copy_from(audio_data);
    let array = js_sys::Array::new();
    array.push(&uint8_array);
    let blob_options = BlobPropertyBag::new();
    blob_options.set_type("audio/wav");
    if let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&array, &blob_options) {
        if let Ok(url) = Url::create_object_url_with_blob(&blob) {
            return url;
        }
    }
    String::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn create_audio_url(_audio_data: &[u8]) -> String {
    String::new()
}

/// Triggers a browser download of in-memory bytes.
#[cfg(target_arch = "wasm32")]
pub fn download_bytes(data: &[u8], mime: &str, file_name: &str) {
    use wasm_bindgen::JsCast;
    use web_sys::{window, Blob, BlobPropertyBag, HtmlAnchorElement, Url};

    let Some(document) = window().and_then(|w| w.document()) else { return };
    let uint8_array = js_sys::Uint8Array::new_with_length(data.len() as u32);
    uint8_array.copy_from(data);
    let array = js_sys::Array::new();
    array.push(&uint8_array);
    let blob_options = BlobPropertyBag::new();
    blob_options.set_type(mime);
    let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&array, &blob_options) else { return };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else { return };
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
