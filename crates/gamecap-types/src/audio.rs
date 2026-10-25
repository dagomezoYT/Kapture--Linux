//! Audio format and PCM blocks. Tracks stay independent until an optional mix.

use std::sync::Arc;

use crate::error::TypesError;
use crate::timestamp::MonotonicTimestamp;

/// Target internal sample rate per the architecture (Hz).
pub const INTERNAL_SAMPLE_RATE_HZ: u32 = 48_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SampleRate(u32);

impl SampleRate {
    pub const INTERNAL: Self = Self(INTERNAL_SAMPLE_RATE_HZ);

    pub fn new(hz: u32) -> Result<Self, TypesError> {
        if hz == 0 {
            return Err(TypesError::InvalidSampleRate(hz));
        }
        Ok(Self(hz))
    }

    pub const fn hz(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChannelCount(u16);

impl ChannelCount {
    pub fn new(count: u16) -> Result<Self, TypesError> {
        if count == 0 {
            return Err(TypesError::InvalidChannelCount(count));
        }
        Ok(Self(count))
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const STEREO: Self = Self(2);
    pub const MONO: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioSampleFormat {
    F32,
    I16,
    I32,
}

impl AudioSampleFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::I16 => "i16",
            Self::I32 => "i32",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioFormat {
    pub sample_rate: SampleRate,
    pub channels: ChannelCount,
    pub sample_format: AudioSampleFormat,
    /// `true` = planar (one channel after another), `false` = interleaved.
    pub planar: bool,
}

impl AudioFormat {
    pub fn pcm_f32_48k_stereo_interleaved() -> Self {
        Self {
            sample_rate: SampleRate::INTERNAL,
            channels: ChannelCount::STEREO,
            sample_format: AudioSampleFormat::F32,
            planar: false,
        }
    }

    pub fn frame_count(&self, sample_storage_len: usize) -> usize {
        let ch = self.channels.get() as usize;
        sample_storage_len.checked_div(ch).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioTrackId(u32);

impl AudioTrackId {
    pub const SYSTEM: Self = Self(1);
    pub const MICROPHONE: Self = Self(2);
    pub const MASTER_MIX: Self = Self(99);

    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioTrackKind {
    System,
    Microphone,
    MasterMix,
}

impl AudioTrackKind {
    pub const fn default_id(self) -> AudioTrackId {
        match self {
            Self::System => AudioTrackId::SYSTEM,
            Self::Microphone => AudioTrackId::MICROPHONE,
            Self::MasterMix => AudioTrackId::MASTER_MIX,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Microphone => "microphone",
            Self::MasterMix => "master_mix",
        }
    }
}

/// One timestamped PCM block belonging to a single track.
#[derive(Clone, Debug)]
pub struct AudioBlock {
    pub block_id: u64,
    pub capture_ts: MonotonicTimestamp,
    pub track_id: AudioTrackId,
    pub kind: AudioTrackKind,
    pub format: AudioFormat,
    /// Interleaved or planar f32 samples. Length is `frames * channels`.
    pub samples: Arc<[f32]>,
}

impl AudioBlock {
    pub fn frame_count(&self) -> usize {
        self.format.frame_count(self.samples.len())
    }
}
