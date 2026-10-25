//! Synthetic 48 kHz audio source with independent tracks.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gamecap_types::{
    media_pts_samples, AudioBlock, AudioError, AudioFormat, AudioTrackKind, INTERNAL_SAMPLE_RATE_HZ,
};

use crate::{AudioCaptureBackend, AudioRequest, AudioSession};

/// 10 ms blocks at 48 kHz.
pub const DEFAULT_BLOCK_FRAMES: usize = 480;

#[derive(Clone, Debug)]
pub struct FakeAudioConfig {
    pub format: AudioFormat,
    pub duration: Duration,
    pub tracks: Vec<AudioTrackKind>,
    pub block_frames: usize,
}

impl FakeAudioConfig {
    pub fn new(duration: Duration) -> Self {
        Self {
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration,
            tracks: vec![AudioTrackKind::System, AudioTrackKind::Microphone],
            block_frames: DEFAULT_BLOCK_FRAMES,
        }
    }

    pub fn expected_blocks_per_track(&self) -> u64 {
        if self.block_frames == 0 {
            return 0;
        }
        let duration_ns = self.duration.as_nanos();
        let total_frames =
            duration_ns.saturating_mul(u128::from(INTERNAL_SAMPLE_RATE_HZ)) / 1_000_000_000u128;
        (total_frames / self.block_frames as u128) as u64
    }
}

pub struct FakeAudioBackend {
    config: FakeAudioConfig,
    started: bool,
    next_block: u64,
    next_track_index: usize,
    expected_blocks: u64,
}

impl FakeAudioBackend {
    pub fn new(config: FakeAudioConfig) -> Self {
        let expected_blocks = config.expected_blocks_per_track();
        Self {
            config,
            started: false,
            next_block: 0,
            next_track_index: 0,
            expected_blocks,
        }
    }

    pub fn expected_blocks_per_track(&self) -> u64 {
        self.expected_blocks
    }

    pub fn expected_total_blocks(&self) -> u64 {
        self.expected_blocks * self.config.tracks.len() as u64
    }
}

impl AudioCaptureBackend for FakeAudioBackend {
    fn start(&mut self, request: AudioRequest) -> Result<AudioSession, AudioError> {
        if request.format.sample_rate.hz() != INTERNAL_SAMPLE_RATE_HZ {
            return Err(AudioError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "sample_rate",
                    format!("FakeAudioBackend requires {} Hz", INTERNAL_SAMPLE_RATE_HZ),
                ),
            ));
        }
        self.config.format = request.format;
        if !request.tracks.is_empty() {
            self.config.tracks = request.tracks.clone();
        }
        if let Some(duration) = request.duration {
            self.config.duration = duration;
            self.expected_blocks = self.config.expected_blocks_per_track();
        }
        self.started = true;
        self.next_block = 0;
        self.next_track_index = 0;
        Ok(AudioSession {
            format: self.config.format,
            tracks: self.config.tracks.clone(),
        })
    }

    fn poll_audio(&mut self, _deadline: Instant) -> Result<Option<AudioBlock>, AudioError> {
        if !self.started {
            return Err(AudioError::InvalidState(
                "poll_audio called before start".to_string(),
            ));
        }
        if self.next_block >= self.expected_blocks {
            return Ok(None);
        }
        let tracks_len = self.config.tracks.len();
        if tracks_len == 0 {
            return Ok(None);
        }
        let kind = self.config.tracks[self.next_track_index];
        let channels = self.config.format.channels.get() as usize;
        let frames = self.config.block_frames;
        let sample_index = self.next_block * frames as u64;
        let pts = media_pts_samples(sample_index, INTERNAL_SAMPLE_RATE_HZ);
        let mut samples = vec![0.0f32; frames * channels];
        // Low-level identity tone: encode block_id in sample 0 as a mantissa-safe integer.
        if !samples.is_empty() {
            samples[0] = (self.next_block as f32) / 1_000_000.0;
        }
        let block = AudioBlock {
            block_id: self.next_block,
            capture_ts: pts,
            track_id: kind.default_id(),
            kind,
            format: self.config.format,
            samples: Arc::from(samples),
        };

        self.next_track_index += 1;
        if self.next_track_index >= tracks_len {
            self.next_track_index = 0;
            self.next_block = self.next_block.saturating_add(1);
        }
        Ok(Some(block))
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.started = false;
        Ok(())
    }

    fn is_complete(&self) -> bool {
        self.started && self.next_block >= self.expected_blocks
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::AudioCaptureBackend;
    use std::time::{Duration, Instant};

    #[test]
    fn ten_seconds_48k_block_count() {
        let cfg = FakeAudioConfig::new(Duration::from_secs(10));
        assert_eq!(cfg.expected_blocks_per_track(), 1_000);
        let mut backend = FakeAudioBackend::new(cfg);
        backend
            .start(AudioRequest {
                tracks: vec![AudioTrackKind::System, AudioTrackKind::Microphone],
                format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
                duration: Some(Duration::from_secs(10)),
            })
            .unwrap();
        let mut n = 0u64;
        while backend
            .poll_audio(Instant::now() + Duration::from_millis(1))
            .unwrap()
            .is_some()
        {
            n += 1;
        }
        assert_eq!(n, 2_000);
        assert!(backend.is_complete());
    }
}
