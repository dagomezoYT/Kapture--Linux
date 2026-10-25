//! Encoded bitstream packets shared by encoder and muxer.

use std::sync::Arc;

use crate::audio::AudioTrackId;
use crate::color::{ColorPrimaries, ColorRange, ColorSpace, HdrMetadata, TransferFunction};
use crate::ids::PacketSeq;
use crate::timestamp::MonotonicTimestamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodecId {
    H264,
    Hevc,
    Av1,
    Aac,
    Opus,
    Pcm,
    /// Timing/identity only; not a real codec.
    Null,
}

impl CodecId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
            Self::Aac => "aac",
            Self::Opus => "opus",
            Self::Pcm => "pcm",
            Self::Null => "null",
        }
    }

    pub const fn is_video(self) -> bool {
        matches!(self, Self::H264 | Self::Hevc | Self::Av1 | Self::Null)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PacketKind {
    Video,
    Audio,
}

/// Color description copied into the container; never dropped silently.
#[derive(Clone, Debug, PartialEq)]
pub struct PacketColorInfo {
    pub color_space: ColorSpace,
    pub transfer: TransferFunction,
    pub primaries: ColorPrimaries,
    pub range: ColorRange,
    pub hdr_metadata: Option<HdrMetadata>,
}

#[derive(Clone, Debug)]
pub struct EncodedPacket {
    pub seq: PacketSeq,
    pub kind: PacketKind,
    pub codec: CodecId,
    pub frame_id: Option<u64>,
    pub audio_track: Option<AudioTrackId>,
    pub pts: MonotonicTimestamp,
    pub dts: Option<MonotonicTimestamp>,
    pub duration_ns: Option<u64>,
    pub is_keyframe: bool,
    pub color: Option<PacketColorInfo>,
    pub data: Arc<[u8]>,
    /// Present on real video packets so the muxer can write track geometry.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl EncodedPacket {
    pub fn video_null(
        seq: PacketSeq,
        frame_id: u64,
        pts: MonotonicTimestamp,
        duration_ns: u64,
        color: PacketColorInfo,
        identity: Arc<[u8]>,
    ) -> Self {
        Self {
            seq,
            kind: PacketKind::Video,
            codec: CodecId::Null,
            frame_id: Some(frame_id),
            audio_track: None,
            pts,
            dts: Some(pts),
            duration_ns: Some(duration_ns),
            is_keyframe: true,
            color: Some(color),
            data: identity,
            width: None,
            height: None,
        }
    }
}
