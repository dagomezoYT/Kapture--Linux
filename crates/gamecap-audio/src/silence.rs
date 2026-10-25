//! Live placeholder that emits silence until the session stops.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gamecap_types::{
    media_pts_samples, AudioBlock, AudioError, AudioFormat, AudioTrackKind, INTERNAL_SAMPLE_RATE_HZ,
};

use crate::{AudioCaptureBackend, AudioRequest, AudioSession, DEFAULT_BLOCK_FRAMES};

/// Produces zeroed PCM so a live session can run without a working audio device.
pub struct SilenceAudioBackend {
    format: AudioFormat,
    tracks: Vec<AudioTrackKind>,
    started: bool,
    next_block: u64,
    next_track: usize,
    origin: Option<Instant>,
    block_frames: usize,
}

impl Default for SilenceAudioBackend {
    fn default() -> Self {
        Self {
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            tracks: vec![AudioTrackKind::System],
            started: false,
            next_block: 0,
            next_track: 0,
            origin: None,
            block_frames: DEFAULT_BLOCK_FRAMES,
        }
    }
}

impl SilenceAudioBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioCaptureBackend for SilenceAudioBackend {
    fn start(&mut self, request: AudioRequest) -> Result<AudioSession, AudioError> {
        if request.tracks.is_empty() {
            return Err(AudioError::InvalidState(
                "silence backend requires at least one track".to_string(),
            ));
        }
        self.format = request.format;
        self.tracks = request.tracks.clone();
        self.started = true;
        self.next_block = 0;
        self.next_track = 0;
        self.origin = Some(Instant::now());
        Ok(AudioSession {
            format: self.format,
            tracks: self.tracks.clone(),
        })
    }

    fn poll_audio(&mut self, deadline: Instant) -> Result<Option<AudioBlock>, AudioError> {
        if !self.started {
            return Err(AudioError::InvalidState(
                "silence backend not started".to_string(),
            ));
        }
        let origin = match self.origin {
            Some(o) => o,
            None => return Ok(None),
        };
        let block_dur = Duration::from_secs_f64(
            self.block_frames as f64 / f64::from(INTERNAL_SAMPLE_RATE_HZ),
        );
        let due = origin + block_dur.saturating_mul(self.next_block as u32);
        let now = Instant::now();
        if now < due {
            if deadline < due {
                return Ok(None);
            }
            let remaining = due.saturating_duration_since(now);
            if remaining > Duration::from_millis(5) {
                return Ok(None);
            }
            std::thread::sleep(remaining);
        }
        let kind = self.tracks[self.next_track];
        let channels = self.format.channels.get() as usize;
        let samples = vec![0.0f32; self.block_frames.saturating_mul(channels)];
        let capture_ts = media_pts_samples(
            self.next_block.saturating_mul(self.block_frames as u64),
            INTERNAL_SAMPLE_RATE_HZ,
        );
        let block = AudioBlock {
            block_id: self.next_block,
            capture_ts,
            track_id: kind.default_id(),
            kind,
            format: self.format,
            samples: Arc::from(samples),
        };
        self.next_track += 1;
        if self.next_track >= self.tracks.len() {
            self.next_track = 0;
            self.next_block = self.next_block.saturating_add(1);
        }
        Ok(Some(block))
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.started = false;
        Ok(())
    }
}
