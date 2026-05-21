//! Result types for domain operations
#![allow(dead_code)]

use super::types::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The success response
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationResult {
    /// Unique identifier for this generation request
    pub request_id: Uuid,
    /// The generated script
    pub script: ListeningScript,
    /// The synthesized audio
    pub audio: AudioTrack,
}

/// The error response
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationFailure {
    /// Category of failure
    pub reason: FailureReason,
    /// User-friendly message
    pub message: String,
}

impl GenerationFailure {
    /// Creates a new failure with the given reason and message
    pub fn new(reason: FailureReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }

    /// Creates an InvalidConfiguration failure
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(FailureReason::InvalidConfiguration, message)
    }

    /// Creates a ScriptGenerationError failure
    pub fn script_error(message: impl Into<String>) -> Self {
        Self::new(FailureReason::ScriptGenerationError, message)
    }

    /// Creates an AudioSynthesisError failure
    pub fn audio_error(message: impl Into<String>) -> Self {
        Self::new(FailureReason::AudioSynthesisError, message)
    }

    /// Creates a SystemError failure
    pub fn system_error(message: impl Into<String>) -> Self {
        Self::new(FailureReason::SystemError, message)
    }
}

impl std::fmt::Display for GenerationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.reason, self.message)
    }
}

impl std::error::Error for GenerationFailure {}

/// Category of domain failure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    /// The request parameters violate section rules
    InvalidConfiguration,
    /// The LLM failed to produce a valid script
    ScriptGenerationError,
    /// The TTS service failed
    AudioSynthesisError,
    /// Unexpected internal error
    SystemError,
}

impl std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailureReason::InvalidConfiguration => write!(f, "Invalid Configuration"),
            FailureReason::ScriptGenerationError => write!(f, "Script Generation Error"),
            FailureReason::AudioSynthesisError => write!(f, "Audio Synthesis Error"),
            FailureReason::SystemError => write!(f, "System Error"),
        }
    }
}
