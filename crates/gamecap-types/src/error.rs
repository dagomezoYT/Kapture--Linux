//! Typed errors shared across GameCap crates.
//!
//! Library code returns these instead of panicking. Callers at the session
//! boundary map them into `SessionState::Failed` with diagnostics.

use std::error::Error;
use std::fmt;

use crate::drop::DropReason;

/// Errors originating from invalid domain values (not I/O).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypesError {
    ZeroDenominator,
    InvalidRational,
    InvalidDimension { width: u32, height: u32 },
    InvalidPixelFormat(&'static str),
    InvalidSampleRate(u32),
    InvalidChannelCount(u16),
}

impl fmt::Display for TypesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDenominator => write!(f, "rational denominator must not be zero"),
            Self::InvalidRational => {
                write!(f, "rational numerator must not be zero in this context")
            }
            Self::InvalidDimension { width, height } => {
                write!(f, "invalid video dimensions {width}x{height}")
            }
            Self::InvalidPixelFormat(name) => write!(f, "invalid pixel format: {name}"),
            Self::InvalidSampleRate(rate) => write!(f, "invalid sample rate: {rate}"),
            Self::InvalidChannelCount(ch) => write!(f, "invalid channel count: {ch}"),
        }
    }
}

impl Error for TypesError {}

/// Capture backend failures. Platform crates must map OS errors into this type
/// and must not leak OS error structs into `gamecap-core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    PermissionDenied(String),
    DeviceLost(String),
    UnsupportedCombination(UnsupportedCombination),
    InvalidState(String),
    Backend(String),
    Timeout(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied(msg) => write!(f, "permission denied: {msg}"),
            Self::DeviceLost(msg) => write!(f, "device lost: {msg}"),
            Self::UnsupportedCombination(inner) => write!(f, "{inner}"),
            Self::InvalidState(msg) => write!(f, "invalid capture state: {msg}"),
            Self::Backend(msg) => write!(f, "capture backend: {msg}"),
            Self::Timeout(msg) => write!(f, "capture timeout: {msg}"),
        }
    }
}

impl Error for CaptureError {}

/// Audio backend failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    PermissionDenied(String),
    DeviceLost(String),
    UnsupportedCombination(UnsupportedCombination),
    InvalidState(String),
    Backend(String),
    Xrun { track: String, gap_ns: u64 },
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied(msg) => write!(f, "audio permission denied: {msg}"),
            Self::DeviceLost(msg) => write!(f, "audio device lost: {msg}"),
            Self::UnsupportedCombination(inner) => write!(f, "{inner}"),
            Self::InvalidState(msg) => write!(f, "invalid audio state: {msg}"),
            Self::Backend(msg) => write!(f, "audio backend: {msg}"),
            Self::Xrun { track, gap_ns } => {
                write!(f, "audio xrun on track {track}, gap {gap_ns} ns")
            }
        }
    }
}

impl Error for AudioError {}

/// Encoder failures, including failed capability matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderError {
    DeviceLost(String),
    UnsupportedCombination(UnsupportedCombination),
    InvalidState(String),
    Backend(String),
    ResetRequired(String),
}

impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceLost(msg) => write!(f, "encoder device lost: {msg}"),
            Self::UnsupportedCombination(inner) => write!(f, "{inner}"),
            Self::InvalidState(msg) => write!(f, "invalid encoder state: {msg}"),
            Self::Backend(msg) => write!(f, "encoder backend: {msg}"),
            Self::ResetRequired(msg) => write!(f, "encoder reset required: {msg}"),
        }
    }
}

impl Error for EncoderError {}

/// Muxer / writer failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxError {
    Io(String),
    InvalidState(String),
    Backend(String),
    DiskBackpressure,
}

impl fmt::Display for MuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "mux I/O: {msg}"),
            Self::InvalidState(msg) => write!(f, "invalid mux state: {msg}"),
            Self::Backend(msg) => write!(f, "mux backend: {msg}"),
            Self::DiskBackpressure => write!(f, "disk backpressure"),
        }
    }
}

impl Error for MuxError {}

/// A requested combination of format / codec / path is not available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCombination {
    pub feature: String,
    pub reason: String,
}

impl fmt::Display for UnsupportedCombination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported combination ({}): {}",
            self.feature, self.reason
        )
    }
}

impl Error for UnsupportedCombination {}

impl UnsupportedCombination {
    pub fn new(feature: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            feature: feature.into(),
            reason: reason.into(),
        }
    }
}

/// Drop classification used by metrics. Never fold these into an average FPS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedDrop {
    pub reason: DropReason,
    pub frame_id: Option<u64>,
    pub detail: String,
}
