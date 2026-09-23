//! Application layer: the use cases the UI calls, as Dioxus `#[server]` functions.
//!
//! In Dioxus 0.7 the body of a `#[server]` function is compiled only into the
//! server binary; the browser gets a stub that performs the HTTP call. So the
//! bodies may use `crate::infrastructure` freely, while the signatures (and
//! any DTO defined here) must be plain serialisable types that compile on both.

pub mod audio;
pub mod exams;
pub mod passages;
pub mod tasks;
pub mod topics;

use dioxus::prelude::ServerFnError;

/// Converts any readable error into the error the browser shows.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub(crate) fn user_error<E: std::fmt::Display>(error: E) -> ServerFnError {
    ServerFnError::new(error.to_string())
}
