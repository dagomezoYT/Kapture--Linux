//! Capture backend trait and negotiated session info.
//!
//! Native OS types stay in platform crates. This crate only describes the
//! contract `gamecap-core` uses.

#![forbid(unsafe_code)]

pub mod fake;

pub use fake::{FakeCaptureBackend, FakeCaptureConfig, TimingMode};

use std::time::Instant;

use gamecap_types::{
    CaptureCaps, CaptureError, CaptureTarget, CursorMode, FrameRateMode, HdrPolicy, SessionId,
    VideoFormat, VideoFrame,
};

/// What the recorder asks a backend to capture.
#[derive(Clone, Debug)]
pub struct CaptureRequest {
    pub target: CaptureTarget,
    pub format: VideoFormat,
    pub frame_rate_mode: FrameRateMode,
    pub cursor: CursorMode,
    pub hdr_policy: HdrPolicy,
}

impl CaptureRequest {
    pub fn synthetic(format: VideoFormat, frame_rate_mode: FrameRateMode) -> Self {
        Self {
            target: CaptureTarget::Synthetic,
            format,
            frame_rate_mode,
            cursor: CursorMode::Exclude,
            hdr_policy: HdrPolicy::Auto,
        }
    }
}

/// Negotiated capture session. The backend remains the poller.
#[derive(Clone, Debug)]
pub struct CaptureSession {
    pub id: SessionId,
    pub format: VideoFormat,
}

/// OS-independent video capture contract.
pub trait VideoCaptureBackend: Send {
    fn capabilities(&self) -> CaptureCaps;

    fn start(&mut self, request: CaptureRequest) -> Result<CaptureSession, CaptureError>;

    /// Poll until `deadline` for the next frame.
    ///
    /// `Ok(None)` means no frame is ready yet. Synthetic backends also return
    /// `Ok(None)` after they have emitted their planned duration; the core uses
    /// [`Self::is_complete`] to distinguish idle vs finished.
    fn poll_frame(&mut self, deadline: Instant) -> Result<Option<VideoFrame>, CaptureError>;

    fn reconfigure(&mut self, request: CaptureRequest) -> Result<(), CaptureError>;

    fn stop(&mut self) -> Result<(), CaptureError>;

    /// Default `false` for live OS backends. Synthetic sources return `true`
    /// when their planned media duration has elapsed.
    fn is_complete(&self) -> bool {
        false
    }
}
