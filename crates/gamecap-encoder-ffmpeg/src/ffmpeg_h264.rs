//! Delimited FFmpeg H.264 encoder (NVENC / VAAPI). Not imported by gamecap-core.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gamecap_encoder::{EncoderConfig, VideoEncoder};
use gamecap_types::{
    CodecId, EncodedPacket, EncoderCaps, EncoderError, EncoderVendor, FrameImportPath, GpuFrameRef,
    MonotonicTimestamp, PacketColorInfo, PacketKind, PacketSeq, PixelFormat, RcMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwKind {
    Nvenc,
    Vaapi,
}

struct PendingFrame {
    pts_ns: u64,
    frame_id: u64,
    width: u32,
    height: u32,
    color: PacketColorInfo,
}

/// H.264 via a dedicated `ffmpeg` process. Frames enter as CPU BGRA.
pub struct FfmpegH264Encoder {
    kind: HwKind,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: Option<JoinHandle<()>>,
    chunks: Option<Receiver<Vec<u8>>>,
    leftover: Vec<u8>,
    prefix: Vec<u8>,
    waiting: VecDeque<PendingFrame>,
    pending: Vec<EncodedPacket>,
    next_seq: u64,
    width: u32,
    height: u32,
    bitrate: u32,
    cq: u32,
    in_pix: PixelFormat,
    target_fps: u32,
    yuv444: bool,
    configured: bool,
}

impl FfmpegH264Encoder {
    pub fn nvenc() -> Self {
        Self::new(HwKind::Nvenc)
    }

    pub fn vaapi() -> Self {
        Self::new(HwKind::Vaapi)
    }

    fn new(kind: HwKind) -> Self {
        Self {
            kind,
            child: None,
            stdin: None,
            reader: None,
            chunks: None,
            leftover: Vec::new(),
            prefix: Vec::new(),
            waiting: VecDeque::new(),
            pending: Vec::new(),
            next_seq: 0,
            width: 0,
            height: 0,
            bitrate: 16_000_000,
            cq: 19,
            in_pix: PixelFormat::Nv12,
            target_fps: 60,
            yuv444: false,
            configured: false,
        }
    }

    pub fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps.clamp(24, 360);
    }

    pub fn set_yuv444(&mut self, enabled: bool) {
        self.yuv444 = enabled && self.kind == HwKind::Nvenc;
    }

    pub fn yuv444_enabled(&self) -> bool {
        self.yuv444
    }

    pub fn nvenc_available() -> bool {
        encoder_listed("h264_nvenc")
            && probe(&[
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=256x256:d=0.05",
                "-frames:v",
                "1",
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-tune",
                "ll",
                "-bf",
                "0",
                "-delay",
                "0",
                "-f",
                "null",
                "-",
            ])
    }

    pub fn nvenc_444_available() -> bool {
        encoder_listed("h264_nvenc")
            && probe(&[
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=256x256:d=0.05,format=bgra",
                "-frames:v",
                "1",
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-tune",
                "hq",
                "-profile:v",
                "high444p",
                "-pix_fmt",
                "yuv444p",
                "-bf",
                "0",
                "-delay",
                "0",
                "-f",
                "null",
                "-",
            ])
    }

    pub fn vaapi_available() -> bool {
        encoder_listed("h264_vaapi") && probe_vaapi_device().is_some()
    }

    fn spawn(&mut self, width: u32, height: u32, bitrate: u32) -> Result<(), EncoderError> {
        self.shutdown();
        let ffmpeg = ffmpeg_bin()?;
        let size = format!("{width}x{height}");
        let br = format!("{bitrate}");
        let cq = format!("{}", self.cq);
        let in_fmt = match self.in_pix {
            PixelFormat::Nv12 => "nv12",
            _ => "bgra",
        };
        let fps = format!("{}", self.target_fps);
        let gop = format!("{}", self.target_fps.saturating_mul(2).max(48));
        let use_444 = self.yuv444 && self.in_pix != PixelFormat::Nv12;
        let mut cmd = Command::new(&ffmpeg);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-sws_flags",
                "neighbor+accurate_rnd+full_chroma_int",
            ]);
        match self.kind {
            HwKind::Nvenc => {
                // Keep 4:4:4 when possible so pixel-art / UI chroma is not 2x2 averaged.
                // Avoid -maxrate (OpenH264 SPS). No spatial/temporal AQ.
                cmd.args([
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    in_fmt,
                    "-video_size",
                    &size,
                    "-framerate",
                    &fps,
                    "-i",
                    "pipe:0",
                    "-an",
                    "-fps_mode",
                    "passthrough",
                    "-c:v",
                    "h264_nvenc",
                    "-preset",
                    "p5",
                    "-tune",
                    "hq",
                    "-delay",
                    "0",
                    "-bf",
                    "0",
                    "-rc",
                    "vbr",
                    "-cq",
                    &cq,
                    "-g",
                    &gop,
                    "-color_range",
                    "pc",
                    "-colorspace",
                    "bt709",
                    "-color_primaries",
                    "bt709",
                    "-color_trc",
                    "bt709",
                ]);
                if use_444 {
                    cmd.args(["-profile:v", "high444p", "-pix_fmt", "yuv444p"]);
                }
                cmd.args(["-f", "h264", "pipe:1"]);
            }
            HwKind::Vaapi => {
                let device = probe_vaapi_device().ok_or_else(|| {
                    EncoderError::Backend("no VAAPI device can encode H.264".to_string())
                })?;
                let hw = format!("vaapi=va:{device}");
                cmd.args([
                    "-init_hw_device",
                    &hw,
                    "-filter_hw_device",
                    "va",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    in_fmt,
                    "-video_size",
                    &size,
                    "-framerate",
                    &fps,
                    "-i",
                    "pipe:0",
                    "-an",
                    "-fps_mode",
                    "passthrough",
                    "-vf",
                    "format=nv12,hwupload",
                    "-c:v",
                    "h264_vaapi",
                    "-bf",
                    "0",
                    "-b:v",
                    &br,
                    "-g",
                    &gop,
                    "-f",
                    "h264",
                    "pipe:1",
                ]);
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| EncoderError::Backend(format!("ffmpeg spawn: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EncoderError::Backend("ffmpeg stdin missing".to_string()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| EncoderError::Backend("ffmpeg stdout missing".to_string()))?;
        let (tx, rx) = mpsc::sync_channel(8);
        let reader = thread::Builder::new()
            .name("gamecap-ffmpeg-out".to_string())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| EncoderError::Backend(e.to_string()))?;
        self.child = Some(child);
        self.stdin = Some(stdin);
        self.reader = Some(reader);
        self.chunks = Some(rx);
        self.leftover.clear();
        self.prefix.clear();
        self.waiting.clear();
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn ensure_alive(&mut self) -> Result<(), EncoderError> {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(EncoderError::DeviceLost(format!("ffmpeg exited: {status}")));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(EncoderError::DeviceLost(format!("ffmpeg wait: {e}")));
                }
            }
        }
        Ok(())
    }

    fn pull_packets(&mut self, eof: bool) -> Result<(), EncoderError> {
        if let Some(rx) = self.chunks.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(chunk) => self.leftover.extend_from_slice(&chunk),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }
        let nals = if eof {
            take_all_nals(&mut self.leftover)
        } else {
            take_complete_nals(&mut self.leftover)
        };
        for nal in nals {
            if nal.is_empty() {
                continue;
            }
            let nal_type = nal[0] & 0x1F;
            if matches!(nal_type, 6 | 7 | 8 | 9) {
                push_start_code(&mut self.prefix);
                self.prefix.extend_from_slice(&nal);
                continue;
            }
            if nal_type != 1 && nal_type != 5 {
                continue;
            }
            let meta = match self.waiting.pop_front() {
                Some(m) => m,
                None => continue,
            };
            let mut data = std::mem::take(&mut self.prefix);
            push_start_code(&mut data);
            data.extend_from_slice(&nal);
            let seq = PacketSeq::from_raw(self.next_seq);
            self.next_seq = self.next_seq.saturating_add(1);
            self.pending.push(EncodedPacket {
                seq,
                kind: PacketKind::Video,
                codec: CodecId::H264,
                frame_id: Some(meta.frame_id),
                audio_track: None,
                pts: MonotonicTimestamp::from_nanos(meta.pts_ns),
                dts: Some(MonotonicTimestamp::from_nanos(meta.pts_ns)),
                duration_ns: None,
                is_keyframe: nal_type == 5,
                color: Some(meta.color),
                data: Arc::from(data),
                width: Some(meta.width),
                height: Some(meta.height),
            });
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        self.stdin = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        self.chunks = None;
    }
}

impl Drop for FfmpegH264Encoder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl VideoEncoder for FfmpegH264Encoder {
    fn capabilities(&self) -> EncoderCaps {
        let (vendor, name) = match self.kind {
            HwKind::Nvenc => (EncoderVendor::Nvidia, "ffmpeg-nvenc"),
            HwKind::Vaapi => (vaapi_vendor(), "ffmpeg-vaapi"),
        };
        EncoderCaps {
            vendor,
            name: name.to_string(),
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
                    format!(
                        "FFmpeg H.264 encoder does not encode {}",
                        cfg.codec.as_str()
                    ),
                ),
            ));
        }
        let width = cfg.format.width & !1;
        let height = cfg.format.height & !1;
        if width < 16 || height < 16 {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "size",
                    format!("encoder needs at least 16x16, got {width}x{height}"),
                ),
            ));
        }
        let bitrate = match cfg.quality {
            gamecap_types::QualityProfile::Performance => 8_000_000,
            gamecap_types::QualityProfile::HighQuality => 16_000_000,
            gamecap_types::QualityProfile::NearLossless => 35_000_000,
            gamecap_types::QualityProfile::Lossless => 50_000_000,
            gamecap_types::QualityProfile::Advanced(ref adv) => adv
                .bitrate_kbps
                .map(|kbps| kbps.saturating_mul(1000))
                .unwrap_or(16_000_000),
        };
        self.cq = match cfg.quality {
            gamecap_types::QualityProfile::Performance => 20,
            gamecap_types::QualityProfile::HighQuality => 16,
            gamecap_types::QualityProfile::NearLossless => 14,
            gamecap_types::QualityProfile::Lossless => 12,
            gamecap_types::QualityProfile::Advanced(_) => 16,
        };
        if self.yuv444 {
            self.cq = match cfg.quality {
                gamecap_types::QualityProfile::Performance => 18,
                gamecap_types::QualityProfile::HighQuality => 12,
                gamecap_types::QualityProfile::NearLossless => 10,
                gamecap_types::QualityProfile::Lossless => 8,
                gamecap_types::QualityProfile::Advanced(_) => 12,
            };
        }
        self.bitrate = bitrate;
        self.width = width;
        self.height = height;
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
                "FFmpeg adapter only accepts CPU frames",
            ))
        })?;
        let width = video.width & !1;
        let height = video.height & !1;
        if width < 16 || height < 16 {
            return Err(EncoderError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "size",
                    format!("encoder needs at least 16x16, got {width}x{height}"),
                ),
            ));
        }
        if self.child.is_none()
            || width != self.width
            || height != self.height
            || video.pixel_format != self.in_pix
        {
            self.in_pix = match video.pixel_format {
                PixelFormat::Nv12 | PixelFormat::Bgra8 => video.pixel_format,
                other => {
                    return Err(EncoderError::UnsupportedCombination(
                        gamecap_types::UnsupportedCombination::new(
                            "pixel_format",
                            format!(
                                "FFmpeg adapter expects nv12 or bgra8, got {}",
                                other.as_str()
                            ),
                        ),
                    ));
                }
            };
            self.spawn(width, height, self.bitrate)?;
        }
        self.ensure_alive()?;
        let expected = self
            .in_pix
            .buffer_size(width, height)
            .map_err(|e| EncoderError::Backend(e.to_string()))?;
        let packed_stride = match self.in_pix {
            PixelFormat::Nv12 => width as usize,
            _ => width as usize * 4,
        };
        let stride = match &video.storage {
            gamecap_types::FrameStorage::Cpu { stride, .. } => *stride,
            _ => packed_stride,
        };
        let stdin = match self.stdin.as_mut() {
            Some(s) => s,
            None => {
                return Err(EncoderError::DeviceLost("ffmpeg stdin closed".to_string()));
            }
        };
        if self.in_pix == PixelFormat::Nv12 {
            if bytes.len() < expected {
                return Err(EncoderError::Backend("short NV12 frame".to_string()));
            }
            stdin
                .write_all(&bytes[..expected])
                .map_err(|e| EncoderError::DeviceLost(format!("ffmpeg stdin: {e}")))?;
        } else if stride == packed_stride && bytes.len() >= expected {
            stdin
                .write_all(&bytes[..expected])
                .map_err(|e| EncoderError::DeviceLost(format!("ffmpeg stdin: {e}")))?;
        } else {
            for row in 0..height as usize {
                let start = row.saturating_mul(stride);
                let end = start.saturating_add(packed_stride);
                if end > bytes.len() {
                    return Err(EncoderError::Backend("short CPU frame".to_string()));
                }
                stdin
                    .write_all(&bytes[start..end])
                    .map_err(|e| EncoderError::DeviceLost(format!("ffmpeg stdin: {e}")))?;
            }
        }
        self.waiting.push_back(PendingFrame {
            pts_ns: video.capture_ts.as_nanos(),
            frame_id: video.frame_id,
            width,
            height,
            color: PacketColorInfo {
                color_space: video.color_space,
                transfer: video.transfer,
                primaries: video.primaries,
                range: video.range,
                hdr_metadata: video.hdr_metadata.clone(),
            },
        });
        self.pull_packets(false)
    }

    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.pull_packets(false)?;
        Ok(std::mem::take(&mut self.pending))
    }

    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.stdin = None;
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        self.pull_packets(true)?;
        if let Some(mut child) = self.child.take() {
            wait_child(&mut child, Duration::from_secs(2));
        }
        Ok(std::mem::take(&mut self.pending))
    }
}

fn ffmpeg_bin() -> Result<String, EncoderError> {
    if which("ffmpeg").is_some() {
        return Ok("ffmpeg".to_string());
    }
    if Path::new("/usr/bin/ffmpeg").is_file() {
        return Ok("/usr/bin/ffmpeg".to_string());
    }
    Err(EncoderError::Backend("ffmpeg not found".to_string()))
}

fn which(bin: &str) -> Option<()> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        if Path::new(dir).join(bin).is_file() {
            return Some(());
        }
    }
    None
}

fn encoder_listed(name: &str) -> bool {
    let ffmpeg = match ffmpeg_bin() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let out = match Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    String::from_utf8_lossy(&out.stdout).contains(name)
}

fn probe(args: &[&str]) -> bool {
    let ffmpeg = match ffmpeg_bin() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    run_with_timeout(&mut cmd, Duration::from_secs(8))
}

fn probe_vaapi_device() -> Option<String> {
    let ffmpeg = ffmpeg_bin().ok()?;
    for device in vaapi_render_nodes() {
        let hw = format!("vaapi=va:{device}");
        let mut cmd = Command::new(&ffmpeg);
        cmd.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-init_hw_device",
            &hw,
            "-filter_hw_device",
            "va",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=256x256:d=0.05",
            "-frames:v",
            "1",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            "h264_vaapi",
            "-bf",
            "0",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        if run_with_timeout(&mut cmd, Duration::from_secs(8)) {
            return Some(device);
        }
    }
    None
}

fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> bool {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(_) => return false,
        }
    }
}

fn wait_child(child: &mut Child, timeout: Duration) {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return,
        }
    }
}

fn vaapi_vendor() -> EncoderVendor {
    match gamecap_hardware::display_vendor() {
        gamecap_hardware::GpuVendor::Intel => EncoderVendor::Intel,
        gamecap_hardware::GpuVendor::Nvidia => EncoderVendor::Nvidia,
        // Unknown stays on the historical label so a failed probe does not
        // reclassify a working VA-API encoder as software.
        gamecap_hardware::GpuVendor::Amd | gamecap_hardware::GpuVendor::Other => EncoderVendor::Amd,
    }
}

fn vaapi_render_nodes() -> Vec<String> {
    let dir = match std::fs::read_dir("/dev/dri") {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut nodes: Vec<String> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("renderD"))
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    nodes.sort();
    prefer_render_node(nodes, gamecap_hardware::display_render_node().as_deref())
}

/// Put the display GPU's render node first. Other nodes keep their order.
fn prefer_render_node(mut nodes: Vec<String>, preferred: Option<&str>) -> Vec<String> {
    let Some(preferred) = preferred else {
        return nodes;
    };
    if let Some(pos) = nodes.iter().position(|node| node == preferred) {
        let node = nodes.remove(pos);
        nodes.insert(0, node);
    }
    nodes
}

fn push_start_code(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&[0, 0, 0, 1]);
}

fn take_complete_nals(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let starts = find_start_codes(buf);
    if starts.len() < 2 {
        return Vec::new();
    }
    let last_sc = starts[starts.len() - 1].0;
    let mut nals = Vec::new();
    for w in starts.windows(2) {
        let payload = w[0].0 + w[0].1;
        let end = w[1].0;
        if payload < end {
            nals.push(buf[payload..end].to_vec());
        }
    }
    buf.drain(..last_sc);
    nals
}

fn take_all_nals(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut nals = take_complete_nals(buf);
    let starts = find_start_codes(buf);
    if let Some((sc, len)) = starts.first().copied() {
        let payload = sc + len;
        if payload < buf.len() {
            nals.push(buf[payload..].to_vec());
        }
        buf.clear();
    }
    nals
}

fn find_start_codes(buf: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 2 < buf.len() {
        if i + 3 < buf.len() && buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 0 && buf[i + 3] == 1
        {
            starts.push((i, 4));
            i += 4;
            continue;
        }
        if buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 1 {
            starts.push((i, 3));
            i += 3;
            continue;
        }
        i += 1;
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_annexb_access_units() {
        let mut buf = vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1];
        let nals = take_complete_nals(&mut buf);
        assert_eq!(nals.len(), 2);
        assert_eq!(nals[0][0], 0x67);
        assert_eq!(nals[1][0], 0x65);
        assert_eq!(buf, vec![0, 0, 0, 1]);
    }

    #[test]
    fn display_render_node_is_probed_first() {
        let nodes = vec![
            "/dev/dri/renderD128".to_string(),
            "/dev/dri/renderD129".to_string(),
        ];
        let ordered = prefer_render_node(nodes, Some("/dev/dri/renderD129"));
        assert_eq!(
            ordered,
            vec![
                "/dev/dri/renderD129".to_string(),
                "/dev/dri/renderD128".to_string(),
            ]
        );
    }

    #[test]
    fn missing_preferred_node_keeps_probe_order() {
        let nodes = vec!["/dev/dri/renderD128".to_string()];
        let ordered = prefer_render_node(nodes.clone(), Some("/dev/dri/renderD129"));
        assert_eq!(ordered, nodes);
    }

    #[test]
    fn drain_emits_last_nal() {
        let mut buf = vec![0, 0, 0, 1, 0x65, 0x11, 0x22];
        let nals = take_all_nals(&mut buf);
        assert_eq!(nals, vec![vec![0x65, 0x11, 0x22]]);
        assert!(buf.is_empty());
    }
}
