//! Application layer: the use cases the UI calls, as Dioxus `#[server]` functions.
//!
//! In Dioxus 0.7 the body of a `#[server]` function is compiled only into the
//! server binary; the browser gets a stub that performs the HTTP call. So the
//! bodies may use `crate::infrastructure` freely, while the signatures (and
//! any DTO defined here) must be plain serialisable types that compile on both.

pub mod audio;
pub mod exams;
pub mod naming;
pub mod passages;
pub mod settings;
pub mod tasks;
pub mod topics;
pub mod usage;
pub mod voices;

use dioxus::prelude::ServerFnError;

/// Converts any readable error into the error the browser shows.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub(crate) fn user_error<E: std::fmt::Display>(error: E) -> ServerFnError {
    ServerFnError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    /// Offset of the next line that is a `#[server]` attribute.
    fn next_server_attribute(text: &str) -> Option<usize> {
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if line.trim_start().starts_with("#[server") {
                return Some(offset);
            }
            offset += line.len();
        }
        None
    }

    /// Every server function asks `ingress::require_app_access` before it
    /// awaits anything or spends a rate limit, so a new one cannot forget it.
    #[test]
    fn every_server_function_checks_app_access() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application");
        let mut checked = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            let mut rest = source.as_str();
            while let Some(at) = next_server_attribute(rest) {
                let function = &rest[at..];
                let end = function.find("\n}\n").unwrap_or(function.len());
                let body = &function[..end];
                let name = body
                    .split("fn ")
                    .nth(1)
                    .and_then(|s| s.split('(').next())
                    .unwrap_or("?")
                    .trim()
                    .to_string();
                let guard = body.find("require_app_access()").unwrap_or_else(|| {
                    panic!(
                        "{}: {name} never calls ingress::require_app_access",
                        path.display()
                    )
                });
                for later in [".await", "rate_limiter::check"] {
                    if let Some(at) = body.find(later) {
                        assert!(guard < at, "{name}: {later} comes before the access check");
                    }
                }
                checked.push(name);
                rest = &function[end..];
            }
        }
        assert!(checked.len() >= 20, "{checked:?}");
    }
}
