//! Safety: disk, memory, forbidden hardware mutation, fail-safe stop.

#![allow(clippy::unwrap_used)]

use std::thread;
use std::time::Duration;

use gamecap_audio::{AudioRequest, FakeAudioBackend, FakeAudioConfig};
use gamecap_capture::{CaptureRequest, FakeCaptureBackend, FakeCaptureConfig};
use gamecap_config::{QueueConfig, ResourceLimits, SessionConfig};
use gamecap_core::RecorderSession;
use gamecap_encoder::{EncoderConfig, NullEncoder, VideoEncoder};
use gamecap_hardware::{deny_hardware_mutation, is_forbidden_write_path, ControllableStorageProbe};
use gamecap_mux::NullMuxer;
use gamecap_types::{
    AudioFormat, AudioTrackKind, EncodedPacket, EncoderCaps, EncoderError, ForbiddenOperation,
    FrameRateMode, GpuFrameRef, PixelFormat, Rational, SafetyStopReason,
    SessionState, VideoFormat,
};

use std::path::Path;

fn tiny_limits() -> ResourceLimits {
    ResourceLimits {
        max_pipeline_bytes: 256 * 1024 * 1024,
        min_free_bytes: 1_024,
        max_output_bytes: 64 * 1024 * 1024,
        stop_after_saturated_samples: 2,
        gpu_busy_stop_percent: 97,
        require_disk_probe: true,
    }
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
fn refuses_to_start_without_free_space() {
    let probe = ControllableStorageProbe::new(10);
    let fps = Rational::new(60, 1).unwrap();
    let format = VideoFormat::sdr_rec709(16, 16, PixelFormat::Bgra8).unwrap();
    let duration = Duration::from_secs(1);
    let config = SessionConfig::builder()
        .queues(QueueConfig::new(8, 8, 8).unwrap())
        .limits(tiny_limits())
        .duration(duration)
        .build();
    let mut session = RecorderSession::builder(config)
        .video_capture(FakeCaptureBackend::new(FakeCaptureConfig::new(
            format.clone(),
            fps,
            duration,
        )))
        .audio_capture(FakeAudioBackend::new(FakeAudioConfig::new(duration)))
        .video_encoder(NullEncoder::new())
        .muxer(NullMuxer::new())
        .capture_request(CaptureRequest::synthetic(
            format,
            FrameRateMode::Custom { fps },
        ))
        .audio_request(AudioRequest {
            tracks: vec![AudioTrackKind::System],
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration: Some(duration),
        })
        .storage_probe(probe)
        .build();
    let err = session.start().unwrap_err();
    assert!(err.to_string().contains("insufficient free space"));
}

#[test]
fn shrinking_disk_stops_recording_without_hanging() {
    let probe = ControllableStorageProbe::new(10_000_000);
    let fps = Rational::new(60, 1).unwrap();
    let format = VideoFormat::sdr_rec709(16, 16, PixelFormat::Bgra8).unwrap();
    let duration = Duration::from_secs(2);
    let config = SessionConfig::builder()
        .queues(QueueConfig::new(8, 8, 16).unwrap())
        .limits(tiny_limits())
        .duration(duration)
        .write_sidecar(false)
        .build();
    let mut session = RecorderSession::builder(config)
        .video_capture(FakeCaptureBackend::new(FakeCaptureConfig::new(
            format.clone(),
            fps,
            duration,
        )))
        .audio_capture(FakeAudioBackend::new(FakeAudioConfig::new(duration)))
        .video_encoder(SlowEncoder {
            inner: NullEncoder::new(),
            delay: Duration::from_millis(15),
        })
        .muxer(NullMuxer::new())
        .capture_request(CaptureRequest::synthetic(
            format,
            FrameRateMode::Custom { fps },
        ))
        .audio_request(AudioRequest {
            tracks: vec![AudioTrackKind::System],
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration: Some(duration),
        })
        .storage_probe(probe.clone())
        .build();
    session.start().unwrap();
    probe.set_remaining(0);
    let outcome = session.wait().unwrap();
    assert_eq!(outcome.state, SessionState::Completed);
    assert_eq!(outcome.safety_stop, Some(SafetyStopReason::DiskSpace));
}

#[test]
fn memory_budget_classifies_drops_instead_of_growing() {
    let fps = Rational::new(60, 1).unwrap();
    let format = VideoFormat::sdr_rec709(64, 36, PixelFormat::Bgra8).unwrap();
    let duration = Duration::from_secs(1);
    let mut limits = tiny_limits();
    limits.max_pipeline_bytes = 8;
    limits.min_free_bytes = 1;
    let probe = ControllableStorageProbe::new(10_000_000);
    let config = SessionConfig::builder()
        .queues(QueueConfig::new(32, 32, 32).unwrap())
        .limits(limits)
        .duration(duration)
        .write_sidecar(false)
        .build();
    let mut session = RecorderSession::builder(config)
        .video_capture(FakeCaptureBackend::new(FakeCaptureConfig::new(
            format.clone(),
            fps,
            duration,
        )))
        .audio_capture(FakeAudioBackend::new(FakeAudioConfig::new(duration)))
        .video_encoder(NullEncoder::new())
        .muxer(NullMuxer::new())
        .capture_request(CaptureRequest::synthetic(
            format,
            FrameRateMode::Custom { fps },
        ))
        .audio_request(AudioRequest {
            tracks: vec![AudioTrackKind::System],
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration: Some(duration),
        })
        .storage_probe(probe)
        .build();
    session.start().unwrap();
    let outcome = session.wait().unwrap();
    assert!(
        outcome.metrics.drops.memory_limit > 0,
        "memory budget must drop frames, got {:?}",
        outcome.metrics.drops
    );
    assert!(outcome.metrics.frames_encoded < 60);
}

#[test]
fn hardware_mutation_and_raw_writes_are_impossible() {
    for op in [
        ForbiddenOperation::SetVoltage,
        ForbiddenOperation::SetClock,
        ForbiddenOperation::SetPowerLimit,
        ForbiddenOperation::SetFanCurve,
        ForbiddenOperation::WriteFirmware,
        ForbiddenOperation::WriteBios,
        ForbiddenOperation::WriteSysfs,
        ForbiddenOperation::RawDeviceIo,
        ForbiddenOperation::AutoOverclock,
    ] {
        assert!(deny_hardware_mutation(op).is_err());
    }
    assert!(is_forbidden_write_path(Path::new(
        "/sys/class/hwmon/hwmon0/pwm1"
    )));
    assert!(is_forbidden_write_path(Path::new("/dev/nvme0n1")));
}
