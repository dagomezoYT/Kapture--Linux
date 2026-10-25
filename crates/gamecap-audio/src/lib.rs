//! Audio capture traits and 48 kHz synthetic backend.

#![forbid(unsafe_code)]

pub mod encoder;
pub mod fake;
pub mod resample;
pub mod silence;

pub use encoder::{AudioEncoder, NullAudioEncoder, PcmPassthroughEncoder};
pub use fake::{FakeAudioBackend, FakeAudioConfig, DEFAULT_BLOCK_FRAMES};
pub use silence::SilenceAudioBackend;
pub use resample::resample_to_internal;

use std::time::Instant;

use gamecap_types::{AudioBlock, AudioError, AudioFormat, AudioTrackKind};

#[derive(Clone, Debug)]
pub struct AudioRequest {
    pub tracks: Vec<AudioTrackKind>,
    pub format: AudioFormat,
    /// Media duration for synthetic backends. Live backends ignore this.
    pub duration: Option<std::time::Duration>,
}

#[derive(Clone, Debug)]
pub struct AudioSession {
    pub format: AudioFormat,
    pub tracks: Vec<AudioTrackKind>,
}

pub trait AudioCaptureBackend: Send {
    fn start(&mut self, request: AudioRequest) -> Result<AudioSession, AudioError>;
    fn poll_audio(&mut self, deadline: Instant) -> Result<Option<AudioBlock>, AudioError>;
    fn stop(&mut self) -> Result<(), AudioError>;
    fn is_complete(&self) -> bool {
        false
    }
}
