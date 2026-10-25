//! Core errors. Native OS error types are mapped by backends before reaching here.

use std::fmt;

use gamecap_types::{AudioError, CaptureError, EncoderError, HardwareError, MuxError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    InvalidTransition { from: String, to: String },
    MissingBackend(&'static str),
    Capture(CaptureError),
    Audio(AudioError),
    Encoder(EncoderError),
    Mux(MuxError),
    Join(String),
    Invariant(String),
    Config(String),
    Safety(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid session transition {from} -> {to}")
            }
            Self::MissingBackend(name) => write!(f, "missing backend: {name}"),
            Self::Capture(e) => write!(f, "{e}"),
            Self::Audio(e) => write!(f, "{e}"),
            Self::Encoder(e) => write!(f, "{e}"),
            Self::Mux(e) => write!(f, "{e}"),
            Self::Join(msg) => write!(f, "worker join: {msg}"),
            Self::Invariant(msg) => write!(f, "invariant violated: {msg}"),
            Self::Config(msg) => write!(f, "config: {msg}"),
            Self::Safety(msg) => write!(f, "safety: {msg}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<CaptureError> for CoreError {
    fn from(value: CaptureError) -> Self {
        Self::Capture(value)
    }
}

impl From<AudioError> for CoreError {
    fn from(value: AudioError) -> Self {
        Self::Audio(value)
    }
}

impl From<EncoderError> for CoreError {
    fn from(value: EncoderError) -> Self {
        Self::Encoder(value)
    }
}

impl From<MuxError> for CoreError {
    fn from(value: MuxError) -> Self {
        Self::Mux(value)
    }
}

impl From<HardwareError> for CoreError {
    fn from(value: HardwareError) -> Self {
        Self::Safety(value.to_string())
    }
}
