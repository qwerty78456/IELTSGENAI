//! Preferences kept in this browser's `localStorage`, shared by both pages and
//! never sent to the server. Storage that is missing or refused (a private
//! window, blocked site data) reads as the default.

#[cfg(target_arch = "wasm32")]
const AUTO_DOWNLOAD_KEY: &str = "listening-exam-generator.auto-download";

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Download the DOCX and the WAV as soon as each is ready. Off unless the
/// teacher switched it on in this browser.
#[cfg(target_arch = "wasm32")]
pub fn auto_download() -> bool {
    storage()
        .and_then(|s| s.get_item(AUTO_DOWNLOAD_KEY).ok().flatten())
        .as_deref()
        == Some("on")
}

#[cfg(target_arch = "wasm32")]
pub fn set_auto_download(on: bool) {
    if let Some(storage) = storage() {
        let _ = if on {
            storage.set_item(AUTO_DOWNLOAD_KEY, "on")
        } else {
            storage.remove_item(AUTO_DOWNLOAD_KEY)
        };
    }
}

/// The server render has no browser storage: off.
#[cfg(not(target_arch = "wasm32"))]
pub fn auto_download() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_auto_download(_on: bool) {}
