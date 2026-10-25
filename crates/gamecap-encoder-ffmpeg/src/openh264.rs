//! Software H.264 encoder using the system OpenH264 library (CPU upload path).

use std::sync::Arc;

use gamecap_encoder::{EncoderConfig, VideoEncoder};
use gamecap_types::{
    CodecId, EncodedPacket, EncoderCaps, EncoderError, EncoderVendor, FrameImportPath, GpuFrameRef,
    PacketColorInfo, PacketKind, PacketSeq, PixelFormat, RcMode,
};
use openh264::encoder::{
    EncodedBitStream, Encoder, EncoderConfig as OhConfig, RateControlMode, SpsPpsStrategy,
    UsageType,
};
use openh264::formats::YUVSource;
use openh264::OpenH264API;

/// OpenH264 software encoder. Not a GPU path; used until libav is linked.
pub struct OpenH264Encoder {
    encoder: Option<Encoder>,
    pending: Vec<EncodedPacket>,
    next_seq: u64,
    last_pts_ns: Option<u64>,
    width: u32,
    height: u32,
    configured: bool,
    frames_since_idr: u32,
    quality: gamecap_types::QualityProfile,
    yuv: Option<I420>,
}

impl Default for OpenH264Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenH264Encoder {
    pub fn new() -> Self {
        Self {
            encoder: None,
            pending: Vec::new(),
            next_seq: 0,
            last_pts_ns: None,
            width: 0,
            height: 0,
            configured: false,
            frames_since_idr: 0,
            quality: gamecap_types::QualityProfile::HighQuality,
            yuv: None,
        }
    }

    fn reinit(&mut self, width: u32, height: u32) -> Result<(), EncoderError> {
        if width < 16 || height < 16 {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "size",
                    format!("OpenH264 needs at least 16x16, got {width}x{height}"),
                ),
            ));
        }
        let bitrate = bitrate_for(self.quality.clone(), width, height);
        self.reinit_with_bitrate(width, height, bitrate)
    }

    fn reinit_with_bitrate(
        &mut self,
        width: u32,
        height: u32,
        bitrate: u32,
    ) -> Result<(), EncoderError> {
        let api = load_openh264()?;
        let oh_cfg = OhConfig::new()
            .set_bitrate_bps(bitrate)
            .max_frame_rate(60.0)
            .enable_skip_frame(false)
            .usage_type(UsageType::CameraVideoRealTime)
            .rate_control_mode(RateControlMode::Bufferbased)
            .sps_pps_strategy(SpsPpsStrategy::IncreasingId)
            .set_multiple_thread_idc(0);
        let encoder = Encoder::with_api_config(api, oh_cfg)
            .map_err(|e| EncoderError::Backend(format!("OpenH264 init: {e}")))?;
        self.encoder = Some(encoder);
        self.width = width;
        self.height = height;
        self.frames_since_idr = 0;
        self.yuv = Some(I420::allocate(width as usize, height as usize)?);
        Ok(())
    }
}

impl VideoEncoder for OpenH264Encoder {
    fn capabilities(&self) -> EncoderCaps {
        EncoderCaps {
            vendor: EncoderVendor::Software,
            name: "openh264".to_string(),
            codecs: vec![CodecId::H264],
            bit_depths: vec![8],
            max_width: 4096,
            max_height: 2160,
            max_fps: None,
            rc_modes: vec![RcMode::Vbr, RcMode::Cbr],
            import_paths: vec![FrameImportPath::CpuUpload],
            supports_hdr: false,
            zero_copy_compatible: false,
        }
    }

    fn configure(&mut self, cfg: EncoderConfig) -> Result<(), EncoderError> {
        if cfg.codec != CodecId::H264 && cfg.codec != CodecId::Null {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "codec",
                    format!("OpenH264Encoder does not encode {}", cfg.codec.as_str()),
                ),
            ));
        }
        let width = even_dim(cfg.format.width);
        let height = even_dim(cfg.format.height);
        if width < 16 || height < 16 {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "size",
                    format!("OpenH264 needs at least 16x16, got {width}x{height}"),
                ),
            ));
        }
        let bitrate = bitrate_for(cfg.quality.clone(), width, height);
        self.quality = cfg.quality;
        self.reinit_with_bitrate(width, height, bitrate)?;
        self.configured = true;
        self.next_seq = 0;
        self.pending.clear();
        self.last_pts_ns = None;
        self.frames_since_idr = 0;
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
                "OpenH264Encoder only accepts CPU frames (explicit SHM fallback)",
            ))
        })?;
        let width = even_dim(video.width);
        let height = even_dim(video.height);
        if width != self.width || height != self.height || self.encoder.is_none() {
            self.reinit(width, height)?;
        }
        let stride = match &video.storage {
            gamecap_types::FrameStorage::Cpu { stride, .. } => *stride,
            _ if video.pixel_format == PixelFormat::Nv12 => width as usize,
            _ => width as usize * 4,
        };
        if video.pixel_format == PixelFormat::Nv12 {
            let yuv = match self.yuv.as_mut() {
                Some(buf) => buf,
                None => return Err(EncoderError::InvalidState("YUV buffer missing".to_string())),
            };
            nv12_to_i420(bytes, stride, width as usize, height as usize, yuv)?;
        } else if video.pixel_format == PixelFormat::Bgra8
            || video.pixel_format == PixelFormat::Rgba8
        {
            let yuv = match self.yuv.as_mut() {
                Some(buf) => buf,
                None => return Err(EncoderError::InvalidState("YUV buffer missing".to_string())),
            };
            bgra_to_i420(
                bytes,
                stride,
                width as usize,
                height as usize,
                video.pixel_format == PixelFormat::Rgba8,
                yuv,
            )?;
        } else {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "pixel_format",
                    format!(
                        "OpenH264 M1 path expects nv12/bgra8/rgba8, got {}",
                        video.pixel_format.as_str()
                    ),
                ),
            ));
        }
        let data = {
            let encoder = match self.encoder.as_mut() {
                Some(enc) => enc,
                None => return Err(EncoderError::InvalidState("encoder missing".to_string())),
            };
            if self.frames_since_idr >= 120 {
                encoder.force_intra_frame();
                self.frames_since_idr = 0;
            }
            let yuv = match self.yuv.as_ref() {
                Some(buf) => buf,
                None => return Err(EncoderError::InvalidState("YUV buffer missing".to_string())),
            };
            let bitstream = encoder
                .encode(yuv)
                .map_err(|e| EncoderError::Backend(format!("OpenH264 encode: {e}")))?;
            bitstream_bytes(&bitstream)
        };
        if data.is_empty() {
            return Ok(());
        }
        let duration_ns = match self.last_pts_ns {
            Some(prev) => video.capture_ts.as_nanos().saturating_sub(prev),
            None => 0,
        };
        self.last_pts_ns = Some(video.capture_ts.as_nanos());
        let keyframe = self.frames_since_idr == 0 || looks_like_idr(&data);
        self.frames_since_idr = self.frames_since_idr.saturating_add(1);
        let seq = PacketSeq::from_raw(self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        self.pending.push(EncodedPacket {
            seq,
            kind: PacketKind::Video,
            codec: CodecId::H264,
            frame_id: Some(video.frame_id),
            audio_track: None,
            pts: video.capture_ts,
            dts: Some(video.capture_ts),
            duration_ns: Some(duration_ns),
            is_keyframe: keyframe,
            color: Some(PacketColorInfo {
                color_space: video.color_space,
                transfer: video.transfer,
                primaries: video.primaries,
                range: video.range,
                hdr_metadata: video.hdr_metadata.clone(),
            }),
            data: Arc::from(data),
            width: Some(width),
            height: Some(height),
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

struct I420 {
    width: usize,
    height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl I420 {
    fn allocate(width: usize, height: usize) -> Result<Self, EncoderError> {
        if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
            return Err(EncoderError::Backend(
                "YUV conversion requires even dimensions".to_string(),
            ));
        }
        Ok(Self {
            width,
            height,
            y: vec![0u8; width.saturating_mul(height)],
            u: vec![0u8; (width / 2).saturating_mul(height / 2)],
            v: vec![0u8; (width / 2).saturating_mul(height / 2)],
        })
    }
}

impl YUVSource for I420 {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}

fn nv12_to_i420(
    src: &[u8],
    y_stride: usize,
    width: usize,
    height: usize,
    dst: &mut I420,
) -> Result<(), EncoderError> {
    if dst.width != width || dst.height != height {
        return Err(EncoderError::Backend(
            "YUV buffer size mismatch".to_string(),
        ));
    }
    let uv_stride = y_stride;
    let needed = y_stride
        .saturating_mul(height)
        .saturating_add(uv_stride.saturating_mul(height / 2));
    if src.len() < needed {
        return Err(EncoderError::Backend(format!(
            "NV12 frame too small: {} < {needed}",
            src.len()
        )));
    }
    for row in 0..height {
        let src_off = row.saturating_mul(y_stride);
        let dst_off = row.saturating_mul(width);
        dst.y[dst_off..dst_off + width].copy_from_slice(&src[src_off..src_off + width]);
    }
    let uv_base = y_stride.saturating_mul(height);
    let uv_w = width / 2;
    for row in 0..(height / 2) {
        let src_off = uv_base.saturating_add(row.saturating_mul(uv_stride));
        let dst_off = row.saturating_mul(uv_w);
        for col in 0..uv_w {
            dst.u[dst_off + col] = src[src_off + col * 2];
            dst.v[dst_off + col] = src[src_off + col * 2 + 1];
        }
    }
    Ok(())
}

fn bgra_to_i420(
    src: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    rgba: bool,
    dst: &mut I420,
) -> Result<(), EncoderError> {
    if dst.width != width || dst.height != height {
        return Err(EncoderError::Backend(
            "YUV buffer size mismatch".to_string(),
        ));
    }
    let needed = stride.saturating_mul(height);
    if src.len() < needed {
        return Err(EncoderError::Backend(format!(
            "CPU frame too small: {} < {needed}",
            src.len()
        )));
    }
    let y_plane = dst.y.as_mut_slice();
    let u_plane = dst.u.as_mut_slice();
    let v_plane = dst.v.as_mut_slice();
    let uv_stride = width / 2;
    for row in (0..height).step_by(2) {
        let row0 = row * stride;
        let row1 = (row + 1) * stride;
        let y0 = row * width;
        let y1 = (row + 1) * width;
        let uv_row = (row / 2) * uv_stride;
        for col in (0..width).step_by(2) {
            let i00 = row0 + col * 4;
            let i10 = row0 + (col + 1) * 4;
            let i01 = row1 + col * 4;
            let i11 = row1 + (col + 1) * 4;
            let (r00, g00, b00) = sample_rgb(src, i00, rgba);
            let (r10, g10, b10) = sample_rgb(src, i10, rgba);
            let (r01, g01, b01) = sample_rgb(src, i01, rgba);
            let (r11, g11, b11) = sample_rgb(src, i11, rgba);
            y_plane[y0 + col] = luma(r00, g00, b00);
            y_plane[y0 + col + 1] = luma(r10, g10, b10);
            y_plane[y1 + col] = luma(r01, g01, b01);
            y_plane[y1 + col + 1] = luma(r11, g11, b11);
            let r = (u16::from(r00) + u16::from(r10) + u16::from(r01) + u16::from(r11)) / 4;
            let g = (u16::from(g00) + u16::from(g10) + u16::from(g01) + u16::from(g11)) / 4;
            let b = (u16::from(b00) + u16::from(b10) + u16::from(b01) + u16::from(b11)) / 4;
            let uv_index = uv_row + col / 2;
            u_plane[uv_index] = chroma_u(r as u8, g as u8, b as u8);
            v_plane[uv_index] = chroma_v(r as u8, g as u8, b as u8);
        }
    }
    Ok(())
}

#[inline]
fn sample_rgb(src: &[u8], i: usize, rgba: bool) -> (u8, u8, u8) {
    if rgba {
        (src[i], src[i + 1], src[i + 2])
    } else {
        (src[i + 2], src[i + 1], src[i])
    }
}

#[inline]
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((((66 * u16::from(r)) + (129 * u16::from(g)) + (25 * u16::from(b)) + 128) >> 8) + 16) as u8
}

#[inline]
fn chroma_u(r: u8, g: u8, b: u8) -> u8 {
    let v = (((-38 * i16::from(r)) - (74 * i16::from(g)) + (112 * i16::from(b)) + 128) >> 8) + 128;
    v.clamp(0, 255) as u8
}

#[inline]
fn chroma_v(r: u8, g: u8, b: u8) -> u8 {
    let v = (((112 * i16::from(r)) - (94 * i16::from(g)) - (18 * i16::from(b)) + 128) >> 8) + 128;
    v.clamp(0, 255) as u8
}

fn bitstream_bytes(stream: &EncodedBitStream<'_>) -> Vec<u8> {
    stream.to_vec()
}

fn looks_like_idr(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            let (nal_start, step) = if data[i + 2] == 1 {
                (i + 3, 3)
            } else if i + 4 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                (i + 4, 4)
            } else {
                i += 1;
                continue;
            };
            if nal_start < data.len() && (data[nal_start] & 0x1F) == 5 {
                return true;
            }
            i += step;
            continue;
        }
        i += 1;
    }
    false
}

fn even_dim(v: u32) -> u32 {
    v & !1
}

fn bitrate_for(quality: gamecap_types::QualityProfile, width: u32, height: u32) -> u32 {
    let pixels = u64::from(width).saturating_mul(u64::from(height)).max(1);
    let base_1080 = 1920u64 * 1080;
    let scale = pixels as f64 / base_1080 as f64;
    let base = match quality {
        gamecap_types::QualityProfile::Performance => 8_000_000.0,
        gamecap_types::QualityProfile::HighQuality => 16_000_000.0,
        gamecap_types::QualityProfile::NearLossless => 35_000_000.0,
        gamecap_types::QualityProfile::Lossless => 50_000_000.0,
        gamecap_types::QualityProfile::Advanced(ref adv) => adv
            .bitrate_kbps
            .map(|kbps| f64::from(kbps) * 1000.0)
            .unwrap_or(16_000_000.0),
    };
    (base * scale).clamp(2_000_000.0, 80_000_000.0) as u32
}

fn load_openh264() -> Result<OpenH264API, EncoderError> {
    Ok(OpenH264API::from_source())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn even_dims() {
        assert_eq!(even_dim(1920), 1920);
        assert_eq!(even_dim(1367), 1366);
    }

    #[test]
    fn i420_size() {
        let bgra = vec![0u8; 16 * 16 * 4];
        let mut yuv = I420::allocate(16, 16).unwrap();
        bgra_to_i420(&bgra, 16 * 4, 16, 16, false, &mut yuv).unwrap();
        assert_eq!(yuv.y.len(), 256);
        assert_eq!(yuv.u.len(), 64);
    }

    #[test]
    fn nv12_planes_split() {
        let mut nv12 = vec![0u8; 16 * 16 + 16 * 8];
        nv12[0] = 40;
        nv12[16 * 16] = 80;
        nv12[16 * 16 + 1] = 90;
        let mut yuv = I420::allocate(16, 16).unwrap();
        nv12_to_i420(&nv12, 16, 16, 16, &mut yuv).unwrap();
        assert_eq!(yuv.y[0], 40);
        assert_eq!(yuv.u[0], 80);
        assert_eq!(yuv.v[0], 90);
    }
}
