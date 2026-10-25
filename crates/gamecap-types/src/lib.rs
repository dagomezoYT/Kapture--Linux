//! Shared domain types for GameCap.
//!
//! This crate is OS-agnostic on purpose: native GPU/OS handles never appear
//! here as typed system objects. Platform crates map opaque identifiers.

#![forbid(unsafe_code)]

pub mod audio;
pub mod capabilities;
pub mod color;
pub mod drop;
pub mod error;
pub mod frame;
pub mod ids;
pub mod json;
pub mod packet;
pub mod rational;
pub mod safety;
pub mod state;
pub mod timestamp;
pub mod video;

pub use audio::{
    AudioBlock, AudioFormat, AudioSampleFormat, AudioTrackId, AudioTrackKind, ChannelCount,
    SampleRate, INTERNAL_SAMPLE_RATE_HZ,
};
pub use capabilities::{
    AudioDeviceInfo, CapabilityReport, CaptureBackendKind, CaptureCaps, DisplayInfo, EncoderCaps,
    EncoderVendor, FrameImportPath, GpuAdapterInfo, RcMode,
};
pub use color::{
    ColorPrimaries, ColorRange, ColorSpace, HdrMetadata, MasteringDisplay, TransferFunction,
};
pub use drop::DropReason;
pub use error::{
    AudioError, CaptureError, EncoderError, MuxError, TypesError, UnsupportedCombination,
};
pub use frame::{
    DmaBufPlane, FrameStorage, GpuFrameRef, NativeHandle, NativeHandleKind, VideoFrame,
};
pub use ids::{MonitorId, PacketSeq, SessionId, WindowId};
pub use packet::{CodecId, EncodedPacket, PacketColorInfo, PacketKind};
pub use rational::Rational;
pub use safety::{
    ForbiddenOperation, HardwareError, HostSample, HostTelemetry, NamedTemperature,
    ResourceSnapshot, SafetyAction, SafetyStopReason, StorageProbe,
};
pub use state::{DegradedReason, Diagnostics, SessionState};
pub use timestamp::{media_pts, media_pts_samples, MonotonicTimestamp, SourceTimestamp};
pub use video::{
    CaptureTarget, CursorMode, FrameRateMode, HdrPolicy, PixelFormat, QualityProfile, VideoFormat,
};
