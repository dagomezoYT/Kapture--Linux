//! Null muxer: counts packets and records PTS order. No container bytes.

use gamecap_types::{AudioTrackId, EncodedPacket, MuxError, PacketKind};

use crate::{Muxer, MuxerConfig, MuxerOutput};

#[derive(Debug, Default)]
pub struct NullMuxer {
    opened: bool,
    output: MuxerOutput,
    last_video_pts: Option<u64>,
    reorder_count: u64,
}

impl NullMuxer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reorder_count(&self) -> u64 {
        self.reorder_count
    }
}

impl Muxer for NullMuxer {
    fn open(&mut self, cfg: MuxerConfig) -> Result<(), MuxError> {
        self.opened = true;
        self.output = MuxerOutput {
            video_packets: 0,
            audio_packets: 0,
            bytes_written: 0,
            path: cfg.output_path,
            sidecar_path: cfg.sidecar_path,
        };
        self.last_video_pts = None;
        self.reorder_count = 0;
        Ok(())
    }

    fn write_video(&mut self, packet: EncodedPacket) -> Result<(), MuxError> {
        if !self.opened {
            return Err(MuxError::InvalidState("muxer not open".to_string()));
        }
        if packet.kind != PacketKind::Video {
            return Err(MuxError::Backend("expected video packet".to_string()));
        }
        if let Some(prev) = self.last_video_pts {
            if packet.pts.as_nanos() < prev {
                self.reorder_count = self.reorder_count.saturating_add(1);
            }
        }
        self.last_video_pts = Some(packet.pts.as_nanos());
        self.output.video_packets = self.output.video_packets.saturating_add(1);
        self.output.bytes_written = self
            .output
            .bytes_written
            .saturating_add(packet.data.len() as u64);
        Ok(())
    }

    fn write_audio(&mut self, _track: AudioTrackId, packet: EncodedPacket) -> Result<(), MuxError> {
        if !self.opened {
            return Err(MuxError::InvalidState("muxer not open".to_string()));
        }
        self.output.audio_packets = self.output.audio_packets.saturating_add(1);
        self.output.bytes_written = self
            .output
            .bytes_written
            .saturating_add(packet.data.len() as u64);
        Ok(())
    }

    fn finalize(&mut self) -> Result<MuxerOutput, MuxError> {
        if !self.opened {
            return Err(MuxError::InvalidState("muxer not open".to_string()));
        }
        self.opened = false;
        Ok(self.output.clone())
    }
}
