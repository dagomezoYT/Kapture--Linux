//! Null encoder: preserves timestamps and identity without a real codec.

use std::sync::Arc;

use gamecap_types::{
    CodecId, EncodedPacket, EncoderCaps, EncoderError, EncoderVendor, FrameImportPath, GpuFrameRef,
    PacketColorInfo, PacketSeq, RcMode,
};

use crate::{EncoderConfig, VideoEncoder};

#[derive(Debug)]
pub struct NullEncoder {
    next_seq: u64,
    pending: Vec<EncodedPacket>,
    configured: bool,
    last_pts_ns: Option<u64>,
}

impl NullEncoder {
    pub fn new() -> Self {
        Self {
            next_seq: 0,
            pending: Vec::new(),
            configured: false,
            last_pts_ns: None,
        }
    }
}

impl Default for NullEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoEncoder for NullEncoder {
    fn capabilities(&self) -> EncoderCaps {
        EncoderCaps {
            vendor: EncoderVendor::Test,
            name: "null".to_string(),
            codecs: vec![CodecId::Null],
            bit_depths: vec![8, 10],
            max_width: u32::MAX,
            max_height: u32::MAX,
            max_fps: None,
            rc_modes: vec![RcMode::Lossless],
            import_paths: vec![FrameImportPath::CpuUpload],
            supports_hdr: true,
            zero_copy_compatible: false,
        }
    }

    fn configure(&mut self, _cfg: EncoderConfig) -> Result<(), EncoderError> {
        self.configured = true;
        self.next_seq = 0;
        self.pending.clear();
        self.last_pts_ns = None;
        Ok(())
    }

    fn submit(&mut self, frame: &GpuFrameRef<'_>) -> Result<(), EncoderError> {
        if !self.configured {
            return Err(EncoderError::InvalidState(
                "encoder not configured".to_string(),
            ));
        }
        let video = frame.frame();
        let duration_ns = match self.last_pts_ns {
            Some(prev) => video.capture_ts.as_nanos().saturating_sub(prev),
            None => 0,
        };
        self.last_pts_ns = Some(video.capture_ts.as_nanos());

        // Copy at most the 8-byte identity header. Never download GPU frames.
        let identity: Arc<[u8]> = match frame.existing_cpu_bytes() {
            Some(bytes) if bytes.len() >= 8 => Arc::from(&bytes[..8]),
            _ => Arc::from(video.frame_id.to_le_bytes().to_vec()),
        };

        let seq = PacketSeq::from_raw(self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        self.pending.push(EncodedPacket::video_null(
            seq,
            video.frame_id,
            video.capture_ts,
            duration_ns,
            PacketColorInfo {
                color_space: video.color_space,
                transfer: video.transfer,
                primaries: video.primaries,
                range: video.range,
                hdr_metadata: video.hdr_metadata.clone(),
            },
            identity,
        ));
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(std::mem::take(&mut self.pending))
    }

    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.receive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::EncoderConfig;
    use gamecap_types::{
        FrameStorage, MonotonicTimestamp, PixelFormat, QualityProfile, TransferFunction,
        VideoFormat, VideoFrame,
    };

    #[test]
    fn does_not_require_gpu_download() {
        let mut enc = NullEncoder::new();
        let format = VideoFormat::sdr_rec709(16, 16, PixelFormat::Bgra8).unwrap();
        enc.configure(EncoderConfig {
            format: format.clone(),
            codec: CodecId::Null,
            quality: QualityProfile::HighQuality,
            bit_depth: 8,
        })
        .unwrap();
        let frame = VideoFrame {
            frame_id: 7,
            capture_ts: MonotonicTimestamp::from_nanos(1_000),
            source_ts: None,
            width: 16,
            height: 16,
            pixel_format: PixelFormat::Bgra8,
            color_space: format.color_space,
            transfer: TransferFunction::Bt709,
            primaries: format.primaries,
            range: format.range,
            hdr_metadata: None,
            storage: FrameStorage::Cpu {
                bytes: Arc::from(7u64.to_le_bytes().to_vec()),
                stride: 64,
            },
        };
        enc.submit(&GpuFrameRef::new(&frame)).unwrap();
        let packets = enc.receive().unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].frame_id, Some(7));
        assert_eq!(packets[0].data.len(), 8);
    }
}
