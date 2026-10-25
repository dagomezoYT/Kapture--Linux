//! Null audio encoder: wraps PCM as packets without recoding.

use gamecap_types::{AudioBlock, CodecId, EncodedPacket, EncoderError, PacketKind, PacketSeq};

/// Converts PCM blocks into muxer packets. No compression in M0.
#[derive(Debug, Default)]
pub struct NullAudioEncoder {
    next_seq: u64,
    pending: Vec<EncodedPacket>,
    configured: bool,
}

impl NullAudioEncoder {
    pub fn new() -> Self {
        Self::default()
    }
}

pub trait AudioEncoder: Send {
    fn configure(&mut self, format: gamecap_types::AudioFormat) -> Result<(), EncoderError>;
    fn submit(&mut self, block: &AudioBlock) -> Result<(), EncoderError>;
    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError>;
    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError>;
}

/// Copies interleaved PCM into muxer packets (IEEE float or the block's f32 samples).
#[derive(Debug, Default)]
pub struct PcmPassthroughEncoder {
    next_seq: u64,
    pending: Vec<EncodedPacket>,
    configured: bool,
    sample_rate_hz: u32,
}

impl PcmPassthroughEncoder {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioEncoder for PcmPassthroughEncoder {
    fn configure(&mut self, format: gamecap_types::AudioFormat) -> Result<(), EncoderError> {
        self.configured = true;
        self.sample_rate_hz = format.sample_rate.hz();
        self.next_seq = 0;
        self.pending.clear();
        Ok(())
    }

    fn submit(&mut self, block: &AudioBlock) -> Result<(), EncoderError> {
        if !self.configured {
            return Err(EncoderError::InvalidState(
                "audio encoder not configured".to_string(),
            ));
        }
        let mut bytes = Vec::with_capacity(block.samples.len().saturating_mul(4));
        for sample in block.samples.iter() {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let duration_ns = if self.sample_rate_hz == 0 {
            None
        } else {
            let frames = block.frame_count() as u64;
            Some(frames.saturating_mul(1_000_000_000) / u64::from(self.sample_rate_hz))
        };
        let seq = PacketSeq::from_raw(self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        self.pending.push(EncodedPacket {
            seq,
            kind: PacketKind::Audio,
            codec: CodecId::Pcm,
            frame_id: Some(block.block_id),
            audio_track: Some(block.track_id),
            pts: block.capture_ts,
            dts: Some(block.capture_ts),
            duration_ns,
            is_keyframe: true,
            color: None,
            data: std::sync::Arc::from(bytes),
            width: None,
            height: None,
        });
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(std::mem::take(&mut self.pending))
    }

    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.receive()
    }
}

impl AudioEncoder for NullAudioEncoder {
    fn configure(&mut self, _format: gamecap_types::AudioFormat) -> Result<(), EncoderError> {
        self.configured = true;
        Ok(())
    }

    fn submit(&mut self, block: &AudioBlock) -> Result<(), EncoderError> {
        if !self.configured {
            return Err(EncoderError::InvalidState(
                "audio encoder not configured".to_string(),
            ));
        }
        let seq = PacketSeq::from_raw(self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        let header = block.block_id.to_le_bytes();
        self.pending.push(EncodedPacket {
            seq,
            kind: PacketKind::Audio,
            codec: CodecId::Pcm,
            frame_id: Some(block.block_id),
            audio_track: Some(block.track_id),
            pts: block.capture_ts,
            dts: Some(block.capture_ts),
            duration_ns: None,
            is_keyframe: true,
            color: None,
            data: std::sync::Arc::from(header.to_vec()),
            width: None,
            height: None,
        });
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(std::mem::take(&mut self.pending))
    }

    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.receive()
    }
}
