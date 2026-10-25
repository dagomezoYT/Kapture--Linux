//! Pass through Annex-B H.264 produced next to capture (GStreamer NVENC).

use std::sync::Arc;

use gamecap_encoder::{EncoderConfig, VideoEncoder};
use gamecap_types::{
    CodecId, EncodedPacket, EncoderCaps, EncoderError, EncoderVendor, FrameImportPath, GpuFrameRef,
    PacketColorInfo, PacketKind, PacketSeq, RcMode,
};

pub struct AnnexBPassthroughEncoder {
    pending: Vec<EncodedPacket>,
    next_seq: u64,
    configured: bool,
    codec: CodecId,
    vendor: EncoderVendor,
    name: &'static str,
}

impl AnnexBPassthroughEncoder {
    pub fn new() -> Self {
        Self::h264()
    }

    pub fn h264() -> Self {
        Self {
            pending: Vec::new(),
            next_seq: 0,
            configured: false,
            codec: CodecId::H264,
            vendor: EncoderVendor::Nvidia,
            name: "gst-nvh264enc",
        }
    }

    pub fn av1() -> Self {
        Self {
            pending: Vec::new(),
            next_seq: 0,
            configured: false,
            codec: CodecId::Av1,
            vendor: EncoderVendor::Amd,
            name: "gst-vaav1enc",
        }
    }
}

impl Default for AnnexBPassthroughEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoEncoder for AnnexBPassthroughEncoder {
    fn capabilities(&self) -> EncoderCaps {
        EncoderCaps {
            vendor: self.vendor,
            name: self.name.to_string(),
            codecs: vec![self.codec],
            bit_depths: vec![8],
            max_width: 4096,
            max_height: 2160,
            max_fps: None,
            rc_modes: vec![RcMode::Vbr],
            import_paths: vec![FrameImportPath::CpuUpload],
            supports_hdr: false,
            zero_copy_compatible: true,
        }
    }

    fn configure(&mut self, cfg: EncoderConfig) -> Result<(), EncoderError> {
        if cfg.codec != self.codec && cfg.codec != CodecId::Null {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "codec",
                    format!(
                        "Annex-B passthrough does not encode {}",
                        cfg.codec.as_str()
                    ),
                ),
            ));
        }
        self.configured = true;
        self.next_seq = 0;
        self.pending.clear();
        Ok(())
    }

    fn submit(&mut self, frame: &GpuFrameRef<'_>) -> Result<(), EncoderError> {
        if !self.configured {
            return Err(EncoderError::InvalidState(
                "encoder not configured".to_string(),
            ));
        }
        let video = frame.frame();
        let bytes = frame.existing_cpu_bytes().ok_or_else(|| {
            EncoderError::UnsupportedCombination(gamecap_types::UnsupportedCombination::new(
                "storage",
                "Annex-B passthrough expects CPU access units",
            ))
        })?;
        if bytes.len() < 5 {
            return Ok(());
        }
        let seq = PacketSeq::from_raw(self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        self.pending.push(EncodedPacket {
            seq,
            kind: PacketKind::Video,
            codec: self.codec,
            frame_id: Some(video.frame_id),
            audio_track: None,
            pts: video.capture_ts,
            dts: Some(video.capture_ts),
            duration_ns: None,
            is_keyframe: match self.codec {
                CodecId::Av1 => av1_keyframe(bytes),
                _ => annexb_keyframe(bytes),
            },
            color: Some(PacketColorInfo {
                color_space: video.color_space,
                transfer: video.transfer,
                primaries: video.primaries,
                range: video.range,
                hdr_metadata: video.hdr_metadata.clone(),
            }),
            data: Arc::from(bytes.to_vec()),
            width: Some(video.width),
            height: Some(video.height),
        });
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(std::mem::take(&mut self.pending))
    }

    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(std::mem::take(&mut self.pending))
    }
}

fn av1_keyframe(tu: &[u8]) -> bool {
    let mut i = 0usize;
    while i < tu.len() {
        let b = tu[i];
        if b & 0x80 != 0 {
            break;
        }
        let ty = (b >> 3) & 0x0F;
        let ext = (b & 0x04) != 0;
        let has_size = (b & 0x02) != 0;
        let mut n = 1usize;
        if ext {
            n += 1;
        }
        if !has_size || i + n >= tu.len() {
            break;
        }
        let mut size = 0u64;
        let mut shift = 0u32;
        let mut leb = 0usize;
        loop {
            if i + n + leb >= tu.len() {
                return false;
            }
            let sb = tu[i + n + leb];
            size |= u64::from(sb & 0x7F) << shift;
            leb += 1;
            if sb & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return false;
            }
        }
        if ty == 1 {
            return true;
        }
        i += n + leb + size as usize;
    }
    false
}

fn annexb_keyframe(au: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < au.len() {
        let (pos, sc) = if au[i] == 0 && au[i + 1] == 0 && au[i + 2] == 0 && au[i + 3] == 1 {
            (i, 4usize)
        } else if au[i] == 0 && au[i + 1] == 0 && au[i + 2] == 1 {
            (i, 3)
        } else {
            i += 1;
            continue;
        };
        let payload = pos + sc;
        if payload < au.len() && (au[payload] & 0x1F) == 5 {
            return true;
        }
        i = payload;
    }
    false
}
