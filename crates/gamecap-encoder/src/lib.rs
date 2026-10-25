//! Video encoder trait, capability ranking, and null encoder.

#![forbid(unsafe_code)]

pub mod null;
pub mod selection;

pub use crate::null::NullEncoder;
pub use crate::selection::{rank_encoders, EncoderRequest, FrameStorageKind};

use gamecap_types::{
    CodecId, EncodedPacket, EncoderCaps, EncoderError, GpuFrameRef, QualityProfile, VideoFormat,
};

/// Configuration applied after capability probing. Not a GPU/driver name list.
#[derive(Clone, Debug)]
pub struct EncoderConfig {
    pub format: VideoFormat,
    pub codec: CodecId,
    pub quality: QualityProfile,
    pub bit_depth: u8,
}

pub trait VideoEncoder: Send {
    fn capabilities(&self) -> EncoderCaps;
    fn configure(&mut self, cfg: EncoderConfig) -> Result<(), EncoderError>;
    fn submit(&mut self, frame: &GpuFrameRef<'_>) -> Result<(), EncoderError>;
    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError>;
    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError>;
}
