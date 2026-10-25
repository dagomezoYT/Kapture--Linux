//! Minimal Matroska writer for H.264 (AVCC) + PCM IEEE float.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

use gamecap_types::{AudioTrackId, CodecId, EncodedPacket, MuxError, PacketKind};

use crate::{Muxer, MuxerConfig, MuxerOutput};

const ID_EBML: &[u8] = &[0x1A, 0x45, 0xDF, 0xA3];
const ID_EBML_VERSION: &[u8] = &[0x42, 0x86];
const ID_EBML_READ_VERSION: &[u8] = &[0x42, 0xF7];
const ID_EBML_MAX_ID_LENGTH: &[u8] = &[0x42, 0xF2];
const ID_EBML_MAX_SIZE_LENGTH: &[u8] = &[0x42, 0xF3];
const ID_DOCTYPE: &[u8] = &[0x42, 0x82];
const ID_DOCTYPE_VERSION: &[u8] = &[0x42, 0x87];
const ID_DOCTYPE_READ_VERSION: &[u8] = &[0x42, 0x85];
const ID_SEGMENT: &[u8] = &[0x18, 0x53, 0x80, 0x67];
const ID_INFO: &[u8] = &[0x15, 0x49, 0xA9, 0x66];
const ID_TIMESTAMP_SCALE: &[u8] = &[0x2A, 0xD7, 0xB1];
const ID_MUXING_APP: &[u8] = &[0x4D, 0x80];
const ID_WRITING_APP: &[u8] = &[0x57, 0x41];
const ID_DURATION: &[u8] = &[0x44, 0x89];
const ID_TRACKS: &[u8] = &[0x16, 0x54, 0xAE, 0x6B];
const ID_TRACK_ENTRY: &[u8] = &[0xAE];
const ID_TRACK_NUMBER: &[u8] = &[0xD7];
const ID_TRACK_UID: &[u8] = &[0x73, 0xC5];
const ID_TRACK_TYPE: &[u8] = &[0x83];
const ID_CODEC_ID: &[u8] = &[0x86];
const ID_CODEC_PRIVATE: &[u8] = &[0x63, 0xA2];
const ID_VIDEO: &[u8] = &[0xE0];
const ID_PIXEL_WIDTH: &[u8] = &[0xB0];
const ID_PIXEL_HEIGHT: &[u8] = &[0xBA];
const ID_DEFAULT_DURATION: &[u8] = &[0x23, 0xE3, 0x83];
const ID_COLOUR: &[u8] = &[0x55, 0xB0];
const ID_MATRIX_COEFFICIENTS: &[u8] = &[0x55, 0xB1];
const ID_COLOUR_RANGE: &[u8] = &[0x55, 0xB9];
const ID_TRANSFER: &[u8] = &[0x55, 0xBA];
const ID_PRIMARIES: &[u8] = &[0x55, 0xBB];
const ID_AUDIO: &[u8] = &[0xE1];
const ID_SAMPLING_FREQUENCY: &[u8] = &[0xB5];
const ID_CHANNELS: &[u8] = &[0x9F];
const ID_BIT_DEPTH: &[u8] = &[0x62, 0x64];
const ID_CLUSTER: &[u8] = &[0x1F, 0x43, 0xB6, 0x75];
const ID_TIMESTAMP: &[u8] = &[0xE7];
const ID_SIMPLE_BLOCK: &[u8] = &[0xA3];

const TRACK_VIDEO: u64 = 1;
const TRACK_AUDIO_SYSTEM: u64 = 2;
const TRACK_AUDIO_MIC: u64 = 3;
const TIMESTAMP_SCALE_NS: u64 = 1_000;
const CLUSTER_MAX_REL: u64 = 30_000;

/// Writes a playable `.mkv` with H.264 video and optional PCM audio.
pub struct MkvMuxer {
    opened: bool,
    file: Option<File>,
    path: Option<PathBuf>,
    sidecar: Option<PathBuf>,
    header_written: bool,
    pending: Vec<PendingPacket>,
    codec_private: Option<Vec<u8>>,
    width: u32,
    height: u32,
    sample_rate_hz: u32,
    channels: u16,
    cluster: Vec<u8>,
    cluster_ts: u64,
    max_pts_ns: u64,
    duration_offset: Option<u64>,
    has_system_audio: bool,
    has_mic_audio: bool,
    last_video_tick: Option<u64>,
    last_audio_tick: Option<u64>,
    video_codec: CodecId,
    frame_duration_ns: u64,
    output: MuxerOutput,
}

struct PendingPacket {
    track: u64,
    pts_ns: u64,
    keyframe: bool,
    data: Vec<u8>,
}

impl Default for MkvMuxer {
    fn default() -> Self {
        Self::new()
    }
}

impl MkvMuxer {
    pub fn new() -> Self {
        Self {
            opened: false,
            file: None,
            path: None,
            sidecar: None,
            header_written: false,
            pending: Vec::new(),
            codec_private: None,
            width: 0,
            height: 0,
            sample_rate_hz: 48_000,
            channels: 2,
            cluster: Vec::new(),
            cluster_ts: 0,
            max_pts_ns: 0,
            duration_offset: None,
            has_system_audio: false,
            has_mic_audio: false,
            last_video_tick: None,
            last_audio_tick: None,
            video_codec: CodecId::H264,
            frame_duration_ns: 16_666_667,
            output: MuxerOutput::default(),
        }
    }
}

impl Muxer for MkvMuxer {
    fn open(&mut self, cfg: MuxerConfig) -> Result<(), MuxError> {
        let path = match cfg.output_path {
            Some(p) => p,
            None => {
                return Err(MuxError::InvalidState(
                    "MkvMuxer requires output_path".to_string(),
                ))
            }
        };
        if cfg.container != crate::ContainerKind::Mkv {
            return Err(MuxError::Backend(
                "MkvMuxer only writes Matroska".to_string(),
            ));
        }
        let file = File::create(&path).map_err(|e| MuxError::Io(e.to_string()))?;
        self.file = Some(file);
        self.path = Some(path.clone());
        self.sidecar = cfg.sidecar_path.clone();
        self.width = cfg.video_width.unwrap_or(0);
        self.height = cfg.video_height.unwrap_or(0);
        self.sample_rate_hz = cfg.audio_sample_rate_hz.unwrap_or(48_000);
        self.channels = cfg.audio_channels.unwrap_or(2);
        self.opened = true;
        self.header_written = false;
        self.pending.clear();
        self.codec_private = None;
        self.cluster.clear();
        self.max_pts_ns = 0;
        self.duration_offset = None;
        self.has_system_audio = false;
        self.has_mic_audio = false;
        self.last_video_tick = None;
        self.last_audio_tick = None;
        self.video_codec = CodecId::H264;
        self.frame_duration_ns = cfg.default_frame_duration_ns.unwrap_or(16_666_667).max(1);
        self.output = MuxerOutput {
            path: Some(path),
            sidecar_path: cfg.sidecar_path,
            ..MuxerOutput::default()
        };
        Ok(())
    }

    fn write_video(&mut self, packet: EncodedPacket) -> Result<(), MuxError> {
        self.require_open()?;
        if packet.kind != PacketKind::Video {
            return Err(MuxError::Backend("expected video packet".to_string()));
        }
        if matches!(packet.codec, CodecId::Av1 | CodecId::H264) {
            self.video_codec = packet.codec;
        } else if packet.codec != CodecId::Null {
            return Err(MuxError::Backend(format!(
                "unsupported video codec {}",
                packet.codec.as_str()
            )));
        }
        if let Some(w) = packet.width {
            if w > 0 {
                self.width = w;
            }
        }
        if let Some(h) = packet.height {
            if h > 0 {
                self.height = h;
            }
        }
        let payload = match self.video_codec {
            CodecId::Av1 => packet.data.as_ref().to_vec(),
            _ => to_avcc(&packet.data),
        };
        if self.codec_private.is_none() {
            self.codec_private = match self.video_codec {
                CodecId::Av1 => av1_codec_private(&payload),
                _ => avcc_codec_private(&payload),
            };
        }
        self.queue(PendingPacket {
            track: TRACK_VIDEO,
            pts_ns: packet.pts.as_nanos(),
            keyframe: packet.is_keyframe,
            data: payload,
        })?;
        self.output.video_packets = self.output.video_packets.saturating_add(1);
        Ok(())
    }

    fn write_audio(&mut self, track: AudioTrackId, packet: EncodedPacket) -> Result<(), MuxError> {
        self.require_open()?;
        if packet.codec != CodecId::Pcm && packet.codec != CodecId::Null {
            return Err(MuxError::Backend(format!(
                "unsupported audio codec {}",
                packet.codec.as_str()
            )));
        }
        let mkv_track = if track.as_raw() == AudioTrackId::MICROPHONE.as_raw() {
            self.has_mic_audio = true;
            TRACK_AUDIO_MIC
        } else {
            self.has_system_audio = true;
            TRACK_AUDIO_SYSTEM
        };
        self.queue(PendingPacket {
            track: mkv_track,
            pts_ns: packet.pts.as_nanos(),
            keyframe: true,
            data: packet.data.as_ref().to_vec(),
        })?;
        self.output.audio_packets = self.output.audio_packets.saturating_add(1);
        Ok(())
    }

    fn finalize(&mut self) -> Result<MuxerOutput, MuxError> {
        self.require_open()?;
        if !self.header_written {
            if self.codec_private.is_none() {
                self.opened = false;
                self.file = None;
                return Ok(self.output.clone());
            }
            self.write_header()?;
            let pending = std::mem::take(&mut self.pending);
            for pkt in pending {
                self.write_block(pkt)?;
            }
        }
        self.flush_cluster()?;
        self.patch_duration()?;
        self.opened = false;
        if let Some(file) = self.file.as_mut() {
            file.flush().map_err(|e| MuxError::Io(e.to_string()))?;
        }
        self.file = None;
        Ok(self.output.clone())
    }
}

impl MkvMuxer {
    fn require_open(&self) -> Result<(), MuxError> {
        if self.opened {
            Ok(())
        } else {
            Err(MuxError::InvalidState("muxer not open".to_string()))
        }
    }

    fn queue(&mut self, pkt: PendingPacket) -> Result<(), MuxError> {
        self.output.bytes_written = self
            .output
            .bytes_written
            .saturating_add(pkt.data.len() as u64);
        if !self.header_written {
            if self.codec_private.is_some() {
                self.write_header()?;
                let pending = std::mem::take(&mut self.pending);
                for old in pending {
                    self.write_block(old)?;
                }
                self.write_block(pkt)?;
            } else {
                self.pending.push(pkt);
            }
            return Ok(());
        }
        self.write_block(pkt)
    }

    fn write_header(&mut self) -> Result<(), MuxError> {
        let private = match &self.codec_private {
            Some(p) => p.clone(),
            None => return Err(MuxError::Backend("missing H.264 codec private".to_string())),
        };
        if self.width == 0 || self.height == 0 {
            return Err(MuxError::Backend(
                "missing video dimensions for MKV tracks".to_string(),
            ));
        }
        let mut header = Vec::new();
        header.extend(element(ID_EBML, &{
            let mut body = Vec::new();
            body.extend(u_element(ID_EBML_VERSION, 1));
            body.extend(u_element(ID_EBML_READ_VERSION, 1));
            body.extend(u_element(ID_EBML_MAX_ID_LENGTH, 4));
            body.extend(u_element(ID_EBML_MAX_SIZE_LENGTH, 8));
            body.extend(element(ID_DOCTYPE, b"matroska"));
            body.extend(u_element(ID_DOCTYPE_VERSION, 4));
            body.extend(u_element(ID_DOCTYPE_READ_VERSION, 2));
            body
        }));
        header.extend(ID_SEGMENT);
        header.extend(unknown_size_8());

        let duration_elem = element(ID_DURATION, &0f64.to_be_bytes());
        let mut info = Vec::new();
        info.extend(&duration_elem);
        info.extend(u_element(ID_TIMESTAMP_SCALE, TIMESTAMP_SCALE_NS));
        info.extend(element(ID_MUXING_APP, b"GameCap"));
        info.extend(element(ID_WRITING_APP, b"GameCap"));
        let info_elem = element(ID_INFO, &info);
        // Duration payload sits after Info id + size + Duration id + size.
        self.duration_offset = Some(
            (header.len()
                + ID_INFO.len()
                + encode_vint(info.len() as u64).len()
                + ID_DURATION.len()
                + encode_vint(8).len()) as u64,
        );
        header.extend(info_elem);

        let mut tracks = Vec::new();
        tracks.extend(video_track(
            self.width,
            self.height,
            &private,
            self.video_codec,
            self.frame_duration_ns,
        ));
        tracks.extend(audio_track(
            TRACK_AUDIO_SYSTEM,
            self.sample_rate_hz,
            self.channels,
        ));
        if self.has_mic_audio {
            tracks.extend(audio_track(
                TRACK_AUDIO_MIC,
                self.sample_rate_hz,
                self.channels,
            ));
        }
        header.extend(element(ID_TRACKS, &tracks));

        let file = match self.file.as_mut() {
            Some(f) => f,
            None => return Err(MuxError::InvalidState("output file missing".to_string())),
        };
        file.write_all(&header)
            .map_err(|e| MuxError::Io(e.to_string()))?;
        self.header_written = true;
        Ok(())
    }

    fn write_block(&mut self, pkt: PendingPacket) -> Result<(), MuxError> {
        if pkt.pts_ns > self.max_pts_ns {
            self.max_pts_ns = pkt.pts_ns;
        }
        let last = if pkt.track == TRACK_VIDEO {
            &mut self.last_video_tick
        } else {
            &mut self.last_audio_tick
        };
        let mut ts = pkt.pts_ns / TIMESTAMP_SCALE_NS;
        let min_inc = (self.frame_duration_ns / TIMESTAMP_SCALE_NS).max(1);
        if let Some(prev) = *last {
            if ts <= prev {
                ts = prev.saturating_add(min_inc);
            }
        }
        *last = Some(ts);
        let start_new = self.cluster.is_empty()
            || (pkt.keyframe && pkt.track == TRACK_VIDEO)
            || ts.saturating_sub(self.cluster_ts) > CLUSTER_MAX_REL;
        if start_new {
            self.flush_cluster()?;
            self.cluster_ts = ts;
            self.cluster.extend(u_element(ID_TIMESTAMP, ts));
        }
        let rel = i16::try_from(ts.saturating_sub(self.cluster_ts)).unwrap_or(0);
        self.cluster.extend(element(
            ID_SIMPLE_BLOCK,
            &simple_block(pkt.track, rel, pkt.keyframe, &pkt.data),
        ));
        Ok(())
    }

    fn flush_cluster(&mut self) -> Result<(), MuxError> {
        if self.cluster.is_empty() {
            return Ok(());
        }
        let body = std::mem::take(&mut self.cluster);
        let file = match self.file.as_mut() {
            Some(f) => f,
            None => return Err(MuxError::InvalidState("output file missing".to_string())),
        };
        file.write_all(&element(ID_CLUSTER, &body))
            .map_err(|e| MuxError::Io(e.to_string()))?;
        Ok(())
    }

    fn patch_duration(&mut self) -> Result<(), MuxError> {
        let offset = match self.duration_offset {
            Some(o) => o,
            None => return Ok(()),
        };
        let duration_tc =
            self.max_pts_ns.saturating_add(self.frame_duration_ns) as f64 / TIMESTAMP_SCALE_NS as f64;
        let file = match self.file.as_mut() {
            Some(f) => f,
            None => return Ok(()),
        };
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| MuxError::Io(e.to_string()))?;
        file.write_all(&duration_tc.to_be_bytes())
            .map_err(|e| MuxError::Io(e.to_string()))?;
        file.seek(SeekFrom::End(0))
            .map_err(|e| MuxError::Io(e.to_string()))?;
        Ok(())
    }
}

fn video_track(
    width: u32,
    height: u32,
    codec_private: &[u8],
    codec: CodecId,
    frame_duration_ns: u64,
) -> Vec<u8> {
    let codec_id: &[u8] = match codec {
        CodecId::Av1 => b"V_AV1",
        _ => b"V_MPEG4/ISO/AVC",
    };
    let mut colour = Vec::new();
    colour.extend(u_element(ID_MATRIX_COEFFICIENTS, 1));
    colour.extend(u_element(ID_COLOUR_RANGE, 1));
    colour.extend(u_element(ID_TRANSFER, 1));
    colour.extend(u_element(ID_PRIMARIES, 1));
    let mut video = Vec::new();
    video.extend(u_element(ID_PIXEL_WIDTH, u64::from(width)));
    video.extend(u_element(ID_PIXEL_HEIGHT, u64::from(height)));
    video.extend(element(ID_COLOUR, &colour));
    let mut entry = Vec::new();
    entry.extend(u_element(ID_TRACK_NUMBER, TRACK_VIDEO));
    entry.extend(u_element(ID_TRACK_UID, TRACK_VIDEO));
    entry.extend(u_element(ID_TRACK_TYPE, 1));
    entry.extend(element(ID_CODEC_ID, codec_id));
    entry.extend(u_element(ID_DEFAULT_DURATION, frame_duration_ns.max(1)));
    entry.extend(element(ID_CODEC_PRIVATE, codec_private));
    entry.extend(element(ID_VIDEO, &video));
    element(ID_TRACK_ENTRY, &entry)
}

fn audio_track(number: u64, sample_rate_hz: u32, channels: u16) -> Vec<u8> {
    let mut audio = Vec::new();
    audio.extend(element(
        ID_SAMPLING_FREQUENCY,
        &f64::from(sample_rate_hz).to_be_bytes(),
    ));
    audio.extend(u_element(ID_CHANNELS, u64::from(channels)));
    audio.extend(u_element(ID_BIT_DEPTH, 32));
    let mut entry = Vec::new();
    entry.extend(u_element(ID_TRACK_NUMBER, number));
    entry.extend(u_element(ID_TRACK_UID, number));
    entry.extend(u_element(ID_TRACK_TYPE, 2));
    entry.extend(element(ID_CODEC_ID, b"A_PCM/FLOAT/IEEE"));
    entry.extend(element(ID_AUDIO, &audio));
    element(ID_TRACK_ENTRY, &entry)
}

fn simple_block(track: u64, rel: i16, keyframe: bool, payload: &[u8]) -> Vec<u8> {
    let mut block = encode_vint(track);
    block.extend_from_slice(&rel.to_be_bytes());
    let flags = if keyframe { 0x80 } else { 0x00 };
    block.push(flags);
    block.extend_from_slice(payload);
    block
}

fn element(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(id.len() + 8 + body.len());
    out.extend_from_slice(id);
    out.extend(encode_vint(body.len() as u64));
    out.extend_from_slice(body);
    out
}

fn u_element(id: &[u8], value: u64) -> Vec<u8> {
    element(id, &encode_uint(value))
}

fn encode_uint(value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![0];
    }
    let mut bytes = value.to_be_bytes().to_vec();
    while bytes.len() > 1 && bytes[0] == 0 {
        bytes.remove(0);
    }
    bytes
}

fn unknown_size_8() -> Vec<u8> {
    vec![0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
}

fn encode_vint(value: u64) -> Vec<u8> {
    if value <= 0x7E {
        return vec![0x80 | (value as u8)];
    }
    if value <= 0x3FFE {
        let v = 0x4000 | (value as u16);
        return v.to_be_bytes().to_vec();
    }
    if value <= 0x1F_FFFE {
        let v = 0x20_0000 | (value as u32);
        return v.to_be_bytes()[1..].to_vec();
    }
    if value <= 0x0FFF_FFFE {
        let v = 0x1000_0000 | (value as u32);
        return v.to_be_bytes().to_vec();
    }
    if value <= 0x07_FFFF_FFFE {
        let v = 0x08_0000_0000u64 | value;
        return v.to_be_bytes()[3..].to_vec();
    }
    if value <= 0x03FF_FFFF_FFFE {
        let v = 0x04_0000_0000_0000u64 | value;
        return v.to_be_bytes()[2..].to_vec();
    }
    if value <= 0x01_FFFF_FFFF_FFFE {
        let v = 0x02_0000_0000_0000_00u64 | value;
        return v.to_be_bytes()[1..].to_vec();
    }
    let v = 0x01_0000_0000_0000_00u64 | value;
    v.to_be_bytes().to_vec()
}

fn split_nalus(data: &[u8]) -> Vec<&[u8]> {
    if looks_like_avcc(data) {
        return split_avcc(data);
    }
    split_annexb(data)
}

fn looks_like_avcc(data: &[u8]) -> bool {
    if data.len() < 5 {
        return false;
    }
    if data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1]) {
        return false;
    }
    let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    len > 0 && len + 4 <= data.len()
}

fn split_avcc(mut data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    while data.len() >= 4 {
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        data = &data[4..];
        if len == 0 || len > data.len() {
            break;
        }
        nals.push(&data[..len]);
        data = &data[len..];
    }
    nals
}

fn split_annexb(data: &[u8]) -> Vec<&[u8]> {
    let mut sc = Vec::new();
    let mut i = 0;
    while i + 2 < data.len() {
        if i + 3 < data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            sc.push(i);
            i += 4;
            continue;
        }
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            sc.push(i);
            i += 3;
            continue;
        }
        i += 1;
    }
    let mut nals = Vec::new();
    for (idx, start_code) in sc.iter().copied().enumerate() {
        let payload = if start_code + 3 < data.len()
            && data[start_code] == 0
            && data[start_code + 1] == 0
            && data[start_code + 2] == 0
            && data[start_code + 3] == 1
        {
            start_code + 4
        } else {
            start_code + 3
        };
        let end = sc.get(idx + 1).copied().unwrap_or(data.len());
        if payload < end {
            nals.push(&data[payload..end]);
        }
    }
    nals
}

fn to_avcc(data: &[u8]) -> Vec<u8> {
    let nals = split_nalus(data);
    if nals.is_empty() {
        return data.to_vec();
    }
    let mut out = Vec::new();
    for nal in nals {
        let len = nal.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}

fn av1_codec_private(obu_stream: &[u8]) -> Option<Vec<u8>> {
    let seq = av1_sequence_header_obu(obu_stream)?;
    let payload = av1_obu_payload(seq)?;
    let first = *payload.first()?;
    let seq_profile = first >> 5;
    const SEQ_LEVEL_5_0: u8 = 12;
    let mut record = vec![
        0x81,
        (seq_profile << 5) | SEQ_LEVEL_5_0,
        0x0C,
        0x00,
    ];
    record.extend_from_slice(seq);
    Some(record)
}

fn av1_sequence_header_obu(data: &[u8]) -> Option<&[u8]> {
    let mut off = 0usize;
    while off < data.len() {
        let (ty, total) = av1_parse_obu(&data[off..])?;
        if ty == 1 {
            return Some(&data[off..off + total]);
        }
        off += total;
    }
    None
}

fn av1_obu_payload(obu: &[u8]) -> Option<&[u8]> {
    let (_, total) = av1_parse_obu(obu)?;
    if total != obu.len() {
        return None;
    }
    let b = *obu.first()?;
    let ext = (b & 0x04) != 0;
    let mut i = 1usize;
    if ext {
        i += 1;
    }
    let (_, n) = av1_leb128(&obu[i..])?;
    i += n;
    Some(&obu[i..])
}

fn av1_parse_obu(data: &[u8]) -> Option<(u8, usize)> {
    if data.is_empty() {
        return None;
    }
    let b = data[0];
    if b & 0x80 != 0 {
        return None;
    }
    let ty = (b >> 3) & 0x0F;
    let ext = (b & 0x04) != 0;
    let has_size = (b & 0x02) != 0;
    let mut i = 1usize;
    if ext {
        if i >= data.len() {
            return None;
        }
        i += 1;
    }
    if !has_size {
        return None;
    }
    let (size, n) = av1_leb128(&data[i..])?;
    i += n;
    let total = i.checked_add(size as usize)?;
    if total > data.len() {
        return None;
    }
    Some((ty, total))
}

fn av1_leb128(data: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    for (i, b) in data.iter().copied().enumerate() {
        value |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

fn avcc_codec_private(avcc_payload: &[u8]) -> Option<Vec<u8>> {
    let nals = split_avcc(avcc_payload);
    let mut sps = None;
    let mut pps = None;
    for nal in nals {
        if nal.is_empty() {
            continue;
        }
        match nal[0] & 0x1F {
            7 => sps = Some(nal),
            8 => pps = Some(nal),
            _ => {}
        }
    }
    let sps = sps?;
    let pps = pps?;
    if sps.len() < 4 || sps.len() > u16::MAX as usize || pps.len() > u16::MAX as usize {
        return None;
    }
    let mut private = vec![1, sps[1], sps[2], sps[3], 0xFF, 0xE1];
    private.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    private.extend_from_slice(sps);
    private.push(1);
    private.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    private.extend_from_slice(pps);
    Some(private)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn vint_one_byte() {
        assert_eq!(encode_vint(1), vec![0x81]);
        assert_eq!(encode_vint(126), vec![0xFE]);
    }

    #[test]
    fn annexb_roundtrip_avcc() {
        let annexb = [0, 0, 0, 1, 0x67, 0x42, 0xC0, 0x0A, 0, 0, 0, 1, 0x68, 0xCE];
        let avcc = to_avcc(&annexb);
        assert_eq!(&avcc[..4], &[0, 0, 0, 4]);
        assert_eq!(&avcc[4..8], &[0x67, 0x42, 0xC0, 0x0A]);
        assert!(avcc_codec_private(&avcc).is_some());
    }

    #[test]
    fn writes_duration_and_skips_empty_mic_track() {
        use std::fs;
        use std::sync::Arc;

        use gamecap_types::{EncodedPacket, MonotonicTimestamp, PacketKind, PacketSeq, SessionId};

        use crate::{ContainerKind, Muxer, MuxerConfig};

        let dir = std::env::temp_dir().join("gamecap-mux-test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("duration.mkv");
        let _ = fs::remove_file(&path);

        let mut mux = MkvMuxer::new();
        mux.open(MuxerConfig {
            session_id: SessionId::from_raw(1),
            container: ContainerKind::Mkv,
            output_path: Some(path.clone()),
            sidecar_path: None,
            video_width: Some(16),
            video_height: Some(16),
            audio_sample_rate_hz: Some(48_000),
            audio_channels: Some(2),
            default_frame_duration_ns: Some(16_666_667),
        })
        .unwrap();

        let annexb = [
            0, 0, 0, 1, 0x67, 0x42, 0xC0, 0x0A, 0, 0, 0, 1, 0x68, 0xCE, 0, 0, 0, 1, 0x65, 0x88,
        ];
        mux.write_video(EncodedPacket {
            seq: PacketSeq::from_raw(0),
            kind: PacketKind::Video,
            codec: CodecId::H264,
            frame_id: Some(0),
            audio_track: None,
            pts: MonotonicTimestamp::from_nanos(1_000_000_000),
            dts: None,
            duration_ns: None,
            is_keyframe: true,
            color: None,
            data: Arc::from(annexb.as_slice()),
            width: Some(16),
            height: Some(16),
        })
        .unwrap();
        mux.write_audio(
            AudioTrackId::SYSTEM,
            EncodedPacket {
                seq: PacketSeq::from_raw(1),
                kind: PacketKind::Audio,
                codec: CodecId::Pcm,
                frame_id: None,
                audio_track: Some(AudioTrackId::SYSTEM),
                pts: MonotonicTimestamp::from_nanos(1_000_000_000),
                dts: None,
                duration_ns: None,
                is_keyframe: true,
                color: None,
                data: Arc::from(vec![0u8; 8]),
                width: None,
                height: None,
            },
        )
        .unwrap();
        mux.finalize().unwrap();

        let bytes = fs::read(&path).unwrap();
        assert!(bytes.windows(2).any(|w| w == ID_DURATION));
        let dur_id = bytes.windows(2).position(|w| w == ID_DURATION).unwrap();
        let payload = &bytes[dur_id + 3..dur_id + 11];
        let duration = f64::from_be_bytes(payload.try_into().unwrap());
        assert!(duration > 1_000_000.0, "duration was {duration}");
        let mic_uid = u_element(ID_TRACK_UID, TRACK_AUDIO_MIC);
        assert!(
            !bytes
                .windows(mic_uid.len())
                .any(|w| w == mic_uid.as_slice()),
            "empty microphone track should not be declared"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn element_has_id_and_size() {
        let bytes = element(ID_TRACK_NUMBER, &[0x01]);
        assert_eq!(bytes[0], 0xD7);
        assert_eq!(bytes[1], 0x81);
        assert_eq!(bytes[2], 0x01);
    }
}
