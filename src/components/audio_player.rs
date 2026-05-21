use dioxus::prelude::*;
use crate::domain::ListeningSection;

#[component]
pub fn AudioPlayerSection(audio_data: Vec<u8>, section: ListeningSection) -> Element {
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
                    onclick: move |_| {
                        download_audio(&audio_data, &format!("IELTS_Listening_{:?}_Audio.wav", section));
                    },
                    "⬇ Download Audio"
                }
            }
        }
    }
}

// Helper function to create audio blob URL for playback
#[cfg(target_arch = "wasm32")]
fn create_audio_url(audio_data: &[u8]) -> String {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, Url};

    // Create Uint8Array from audio data
    let uint8_array = js_sys::Uint8Array::new_with_length(audio_data.len() as u32);
    uint8_array.copy_from(audio_data);

    // Create blob with audio/wav mime type
    let array = js_sys::Array::new();
    array.push(&uint8_array);

    let mut blob_options = BlobPropertyBag::new();
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

// Helper function to download audio as WAV file
#[cfg(target_arch = "wasm32")]
fn download_audio(audio_data: &[u8], filename: &str) {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url, window};

    if let Some(window) = window() {
        if let Some(document) = window.document() {
            // Create Uint8Array from audio data
            let uint8_array = js_sys::Uint8Array::new_with_length(audio_data.len() as u32);
            uint8_array.copy_from(audio_data);

            // Create blob with audio/wav mime type
            let array = js_sys::Array::new();
            array.push(&uint8_array);

            let mut blob_options = BlobPropertyBag::new();
            blob_options.set_type("audio/wav");

            if let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&array, &blob_options) {
                if let Ok(url) = Url::create_object_url_with_blob(&blob) {
                    // Create temporary anchor element
                    if let Ok(anchor) = document.create_element("a") {
                        let anchor: HtmlAnchorElement = anchor.unchecked_into();
                        anchor.set_href(&url);
                        anchor.set_download(filename);
                        anchor.click();

                        // Clean up
                        let _ = Url::revoke_object_url(&url);
                    }
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn download_audio(_audio_data: &[u8], _filename: &str) {
    // Download not implemented for non-wasm targets
}
