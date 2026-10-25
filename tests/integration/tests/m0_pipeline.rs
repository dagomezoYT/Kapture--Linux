//! Milestone M0 pipeline: synthetic 10s @ 144 FPS must be exact and drop-free.

#![allow(clippy::unwrap_used)]

use std::thread;
use std::time::Duration;

use gamecap_audio::{AudioRequest, FakeAudioBackend, FakeAudioConfig};
use gamecap_capture::{CaptureRequest, FakeCaptureBackend, FakeCaptureConfig};
use gamecap_config::{BackpressurePolicy, QueueConfig, SessionConfig};
use gamecap_core::{RecorderSession, SessionOutcome};
use gamecap_encoder::{EncoderConfig, NullEncoder, VideoEncoder};
use gamecap_mux::NullMuxer;
use gamecap_types::{
    AudioFormat, AudioTrackKind, EncodedPacket, EncoderCaps, EncoderError, FrameRateMode,
    GpuFrameRef, PixelFormat, QualityProfile, Rational, SessionState, VideoFormat,
};

fn run_fake(
    fps: u32,
    duration: Duration,
    video_queue: usize,
    retries: u32,
    encoder: impl VideoEncoder + 'static,
) -> SessionOutcome {
    let fps_r = Rational::new(fps, 1).unwrap();
    let format = VideoFormat::sdr_rec709(64, 36, PixelFormat::Bgra8).unwrap();
    let queues = QueueConfig::new(video_queue, 256, 512).unwrap();
    let config = SessionConfig::builder()
        .queues(queues)
        .backpressure(BackpressurePolicy {
            encode_full_retries: retries,
            packet_send_timeout: Duration::from_millis(200),
        })
        .quality(QualityProfile::HighQuality)
        .frame_rate_mode(FrameRateMode::Custom { fps: fps_r })
        .duration(duration)
        .write_sidecar(true)
        .build();

    let capture = FakeCaptureBackend::new(FakeCaptureConfig::new(format.clone(), fps_r, duration));
    assert_eq!(
        capture.expected_frame_count(),
        u64::from(fps) * duration.as_secs()
    );

    let audio = FakeAudioBackend::new(FakeAudioConfig::new(duration));
    let mut session = RecorderSession::builder(config)
        .video_capture(capture)
        .audio_capture(audio)
        .video_encoder(encoder)
        .muxer(NullMuxer::new())
        .capture_request(CaptureRequest::synthetic(
            format,
            FrameRateMode::Custom { fps: fps_r },
        ))
        .audio_request(AudioRequest {
            tracks: vec![AudioTrackKind::System, AudioTrackKind::Microphone],
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration: Some(duration),
        })
        .build();

    session.start().unwrap();
    session.wait().unwrap()
}

struct SlowEncoder {
    inner: NullEncoder,
    delay: Duration,
}

impl VideoEncoder for SlowEncoder {
    fn capabilities(&self) -> EncoderCaps {
        self.inner.capabilities()
    }

    fn configure(&mut self, cfg: EncoderConfig) -> Result<(), EncoderError> {
        self.inner.configure(cfg)
    }

    fn submit(&mut self, frame: &GpuFrameRef<'_>) -> Result<(), EncoderError> {
        thread::sleep(self.delay);
        self.inner.submit(frame)
    }

    fn receive(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.inner.receive()
    }

    fn drain(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.inner.drain()
    }
}

#[test]
fn ten_seconds_at_144_fps_is_exact_and_drop_free() {
    let outcome = run_fake(
        144,
        Duration::from_secs(10),
        512,
        100_000,
        NullEncoder::new(),
    );
    assert_eq!(outcome.state, SessionState::Completed);
    assert_eq!(outcome.metrics.frames_captured, 1_440);
    assert_eq!(outcome.metrics.frames_encoded, 1_440);
    assert_eq!(outcome.mux.video_packets, 1_440);
    assert_eq!(outcome.metrics.drops.core_caused(), 0);
    assert_eq!(outcome.metrics.drops.total(), 0);
    assert_eq!(outcome.metrics.duplicated, 0);

    // PTS uses integer nanoseconds: interval is floor(1e9/144) or +1 ns from remainder.
    let expected_interval = 1_000_000_000 / 144;
    let max_interval = expected_interval + 1;
    assert!(
        (expected_interval..=max_interval).contains(&outcome.metrics.timing.p50_ns),
        "p50 {} not in {expected_interval}..={max_interval}",
        outcome.metrics.timing.p50_ns
    );
    assert!(
        (expected_interval..=max_interval).contains(&outcome.metrics.timing.p95_ns),
        "p95 {} not in {expected_interval}..={max_interval}",
        outcome.metrics.timing.p95_ns
    );
    assert!(
        (expected_interval..=max_interval).contains(&outcome.metrics.timing.p99_ns),
        "p99 {} not in {expected_interval}..={max_interval}",
        outcome.metrics.timing.p99_ns
    );

    assert_eq!(outcome.metrics.audio_blocks, 2_000);
    assert_eq!(outcome.mux.audio_packets, 2_000);
    assert!(outcome.sidecar_path.is_some());
}

#[test]
fn sixty_and_two_forty_fps_counts() {
    let s60 = run_fake(60, Duration::from_secs(2), 256, 50_000, NullEncoder::new());
    assert_eq!(s60.metrics.frames_encoded, 120);
    assert_eq!(s60.metrics.drops.core_caused(), 0);

    let s240 = run_fake(
        240,
        Duration::from_secs(2),
        512,
        100_000,
        NullEncoder::new(),
    );
    assert_eq!(s240.metrics.frames_encoded, 480);
    assert_eq!(s240.metrics.drops.core_caused(), 0);
}

#[test]
fn slow_encoder_classifies_encode_backpressure() {
    let outcome = run_fake(
        120,
        Duration::from_secs(1),
        2,
        0,
        SlowEncoder {
            inner: NullEncoder::new(),
            delay: Duration::from_millis(8),
        },
    );
    assert!(
        outcome.metrics.drops.encode_backpressure > 0,
        "slow encoder with a tiny queue must classify encode_backpressure, got {:?}",
        outcome.metrics.drops
    );
    assert_eq!(outcome.metrics.drops.source_gap, 0);
    assert!(outcome.metrics.frames_encoded < 120);
}

#[test]
fn no_reorder_in_muxed_video_count() {
    let outcome = run_fake(60, Duration::from_secs(1), 128, 10_000, NullEncoder::new());
    assert_eq!(outcome.mux.video_packets, outcome.metrics.frames_encoded);
    assert_eq!(outcome.metrics.frames_encoded, 60);
}
