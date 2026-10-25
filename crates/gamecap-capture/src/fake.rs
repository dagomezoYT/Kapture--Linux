//! Deterministic / real-time synthetic video source.
//!
//! Used to prove core timing, queues, and drop classification without an OS
//! capture API. Frames embed `frame_id` in the first 8 bytes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gamecap_types::{
    media_pts, CaptureBackendKind, CaptureCaps, CaptureError, CaptureTarget, FrameStorage,
    PixelFormat, Rational, SessionId, SourceTimestamp, VideoFormat, VideoFrame,
};

use crate::{CaptureRequest, CaptureSession, VideoCaptureBackend};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingMode {
    /// Emit frames as fast as `poll_frame` is called, with exact media timestamps.
    Deterministic,
    /// Sleep to match wall-clock FPS. Not used by default M0 tests.
    Realtime,
}

#[derive(Clone, Debug)]
pub struct FakeCaptureConfig {
    pub format: VideoFormat,
    pub fps: Rational,
    pub duration: Duration,
    pub timing: TimingMode,
}

impl FakeCaptureConfig {
    pub fn new(format: VideoFormat, fps: Rational, duration: Duration) -> Self {
        Self {
            format,
            fps,
            duration,
            timing: TimingMode::Deterministic,
        }
    }

    pub fn expected_frame_count(&self) -> u64 {
        let duration_ns = saturate_duration_ns(self.duration);
        self.fps.cycles_in(duration_ns).unwrap_or_default()
    }
}

pub struct FakeCaptureBackend {
    config: FakeCaptureConfig,
    started: bool,
    next_frame_id: u64,
    expected: u64,
    realtime_origin: Option<Instant>,
}

impl FakeCaptureBackend {
    pub fn new(config: FakeCaptureConfig) -> Self {
        let expected = config.expected_frame_count();
        Self {
            config,
            started: false,
            next_frame_id: 0,
            expected,
            realtime_origin: None,
        }
    }

    pub fn expected_frame_count(&self) -> u64 {
        self.expected
    }

    fn build_frame(&self, frame_id: u64) -> Result<VideoFrame, CaptureError> {
        let fmt = &self.config.format;
        let size = fmt
            .pixel_format
            .buffer_size(fmt.width, fmt.height)
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        let stride = fmt
            .pixel_format
            .stride_bytes(fmt.width)
            .map_err(|e| CaptureError::Backend(e.to_string()))?;

        let mut bytes = vec![0u8; size];
        if bytes.len() >= 8 {
            bytes[..8].copy_from_slice(&frame_id.to_le_bytes());
        }
        let checksum = bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
        if bytes.len() > 8 {
            bytes[8] = checksum;
        }

        let pts = media_pts(frame_id, self.config.fps);
        Ok(VideoFrame {
            frame_id,
            capture_ts: pts,
            source_ts: Some(SourceTimestamp::from_master(pts)),
            width: fmt.width,
            height: fmt.height,
            pixel_format: fmt.pixel_format,
            color_space: fmt.color_space,
            transfer: fmt.transfer,
            primaries: fmt.primaries,
            range: fmt.range,
            hdr_metadata: None,
            storage: FrameStorage::Cpu {
                bytes: Arc::from(bytes),
                stride,
            },
        })
    }
}

impl VideoCaptureBackend for FakeCaptureBackend {
    fn capabilities(&self) -> CaptureCaps {
        CaptureCaps {
            backend: CaptureBackendKind::Fake,
            monitors: Vec::new(),
            supports_window_capture: false,
            supports_hdr: false,
            pixel_formats: vec![PixelFormat::Bgra8, PixelFormat::Rgba8],
            max_fps: Some(self.config.fps),
        }
    }

    fn start(&mut self, request: CaptureRequest) -> Result<CaptureSession, CaptureError> {
        if !matches!(request.target, CaptureTarget::Synthetic) {
            return Err(CaptureError::UnsupportedCombination(
                gamecap_types::UnsupportedCombination::new(
                    "target",
                    "FakeCaptureBackend only supports CaptureTarget::Synthetic",
                ),
            ));
        }
        request
            .format
            .validate()
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        self.config.format = request.format.clone();
        if let Some(fps) = request.frame_rate_mode.target_fps() {
            self.config.fps = fps;
            self.expected = self.config.expected_frame_count();
        }
        self.started = true;
        self.next_frame_id = 0;
        self.realtime_origin = Some(Instant::now());
        Ok(CaptureSession {
            id: SessionId::new(),
            format: self.config.format.clone(),
        })
    }

    fn poll_frame(&mut self, deadline: Instant) -> Result<Option<VideoFrame>, CaptureError> {
        if !self.started {
            return Err(CaptureError::InvalidState(
                "poll_frame called before start".to_string(),
            ));
        }
        if self.next_frame_id >= self.expected {
            return Ok(None);
        }

        if self.config.timing == TimingMode::Realtime {
            let origin = match self.realtime_origin {
                Some(o) => o,
                None => Instant::now(),
            };
            let due = media_pts(self.next_frame_id, self.config.fps).as_duration();
            let target = origin + due;
            let now = Instant::now();
            if target > now {
                let wait = target.saturating_duration_since(now);
                let remaining = deadline.saturating_duration_since(Instant::now());
                if wait > remaining {
                    return Ok(None);
                }
                std::thread::sleep(wait);
            }
        }

        let frame = self.build_frame(self.next_frame_id)?;
        self.next_frame_id = self.next_frame_id.saturating_add(1);
        Ok(Some(frame))
    }

    fn reconfigure(&mut self, request: CaptureRequest) -> Result<(), CaptureError> {
        if !self.started {
            return Err(CaptureError::InvalidState(
                "reconfigure called before start".to_string(),
            ));
        }
        request
            .format
            .validate()
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        self.config.format = request.format;
        if let Some(fps) = request.frame_rate_mode.target_fps() {
            self.config.fps = fps;
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.started = false;
        Ok(())
    }

    fn is_complete(&self) -> bool {
        self.started && self.next_frame_id >= self.expected
    }
}

fn saturate_duration_ns(duration: Duration) -> u64 {
    let ns = duration.as_nanos();
    if ns > u128::from(u64::MAX) {
        u64::MAX
    } else {
        ns as u64
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::VideoCaptureBackend;
    use gamecap_types::PixelFormat;
    use std::time::{Duration, Instant};

    #[test]
    fn expected_count_144_fps_10s() {
        let fps = Rational::new(144, 1).unwrap();
        let format = VideoFormat::sdr_rec709(64, 36, PixelFormat::Bgra8).unwrap();
        let cfg = FakeCaptureConfig::new(format, fps, Duration::from_secs(10));
        assert_eq!(cfg.expected_frame_count(), 1_440);
    }

    #[test]
    fn frames_are_strictly_increasing() {
        let fps = Rational::new(60, 1).unwrap();
        let format = VideoFormat::sdr_rec709(16, 16, PixelFormat::Bgra8).unwrap();
        let mut fake = FakeCaptureBackend::new(FakeCaptureConfig::new(
            format.clone(),
            fps,
            Duration::from_secs(1),
        ));
        fake.start(CaptureRequest::synthetic(
            format,
            gamecap_types::FrameRateMode::Custom { fps },
        ))
        .unwrap();
        let mut last = None;
        let deadline = Instant::now() + Duration::from_secs(1);
        while let Some(frame) = fake.poll_frame(deadline).unwrap() {
            if let Some(id) = last {
                assert_eq!(frame.frame_id, id + 1);
            }
            assert_eq!(frame.identity_frame_id(), Some(frame.frame_id));
            last = Some(frame.frame_id);
        }
        assert_eq!(last, Some(59));
        assert!(fake.is_complete());
    }
}
