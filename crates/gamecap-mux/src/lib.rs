//! Muxer contract. Captures to an abstract container; remux is a later stage.

#![forbid(unsafe_code)]

pub mod mkv;
pub mod null;

pub use mkv::MkvMuxer;
pub use null::NullMuxer;

use std::path::PathBuf;

use gamecap_types::{AudioTrackId, EncodedPacket, MuxError, SessionId};

#[derive(Clone, Debug)]
pub struct MuxerConfig {
    pub session_id: SessionId,
    pub container: ContainerKind,
    pub output_path: Option<PathBuf>,
    pub sidecar_path: Option<PathBuf>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    pub audio_sample_rate_hz: Option<u32>,
    pub audio_channels: Option<u16>,
    /// One video frame in nanoseconds, written as Matroska DefaultDuration.
    pub default_frame_duration_ns: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    Mkv,
    Mp4,
}

impl ContainerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MuxerOutput {
    pub video_packets: u64,
    pub audio_packets: u64,
    pub bytes_written: u64,
    pub path: Option<PathBuf>,
    pub sidecar_path: Option<PathBuf>,
}

pub trait Muxer: Send {
    fn open(&mut self, cfg: MuxerConfig) -> Result<(), MuxError>;
    fn write_video(&mut self, packet: EncodedPacket) -> Result<(), MuxError>;
    fn write_audio(&mut self, track: AudioTrackId, packet: EncodedPacket) -> Result<(), MuxError>;
    fn finalize(&mut self) -> Result<MuxerOutput, MuxError>;
}
