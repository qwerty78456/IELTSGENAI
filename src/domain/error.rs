//! Teacher-readable domain errors. Never wrap HTTP status codes or stack traces here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum DomainError {
    /// The command violates a rule before any generation happens.
    #[error("{0}")]
    InvalidRequest(String),
    /// A passage (script) does not satisfy the part it was generated for.
    #[error("{0}")]
    InvalidPassage(String),
    /// A task or one of its items violates its `TaskSpec`.
    #[error("{0}")]
    InvalidTask(String),
}
