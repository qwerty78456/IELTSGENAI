//! Domain layer of the **Listening Assessment Generation** bounded context.
//!
//! Pure types and rules only: no I/O, no framework types, no `#[server]`.
//! Everything here compiles identically for the browser (wasm32) and the
//! server, so the UI can validate and render the same objects the server
//! produces. See `docs/architecture.md` and `docs/domain_model.md`.
#![allow(dead_code)]

pub mod audio;
pub mod commands;
pub mod error;
pub mod exam;
pub mod format;
pub mod passage;
pub mod speaker;
pub mod task;
pub mod validation;

#[allow(unused_imports)] // consumed on the server only
pub use audio::*;
pub use commands::*;
#[allow(unused_imports)]
pub use error::*;
pub use exam::*;
pub use format::*;
pub use passage::*;
pub use speaker::*;
pub use task::*;
pub use validation::*;
