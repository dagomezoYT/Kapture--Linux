//! Session configuration. Defaults avoid hardcoded devices, codecs, and FPS.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::time::Duration;

use gamecap_types::{FrameRateMode, QualityProfile, SessionId};

/// Bounded-queue capacities. These bound RAM, not "how many FPS we claim".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueConfig {
    pub video: usize,
    pub audio: usize,
    pub packets: usize,
}

impl QueueConfig {
    pub fn new(video: usize, audio: usize, packets: usize) -> Result<Self, ConfigError> {
        if video == 0 || audio == 0 || packets == 0 {
            return Err(ConfigError::ZeroQueueCapacity);
        }
        Ok(Self {
            video,
            audio,
            packets,
        })
    }
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            video: 64,
            audio: 64,
            packets: 128,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    ZeroQueueCapacity,
    InvalidDuration,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroQueueCapacity => write!(f, "queue capacity must be greater than zero"),
            Self::InvalidDuration => write!(f, "session duration must be greater than zero"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Memory, disk and saturation limits. These protect the host, not “max FPS”.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Hard cap on bytes held in capture/encode/audio queues combined.
    pub max_pipeline_bytes: u64,
    /// Refuse to start (and later stop) if the output volume has less than this.
    pub min_free_bytes: u64,
    /// Stop writing before filling the disk; 0 means “no extra cap”.
    pub max_output_bytes: u64,
    /// Consecutive saturated samples before Stop (after Degrade).
    pub stop_after_saturated_samples: u32,
    /// GPU busy percent that counts as saturation when telemetry exists.
    pub gpu_busy_stop_percent: u8,
    /// If true, a failed disk probe aborts start instead of guessing.
    pub require_disk_probe: bool,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_pipeline_bytes: 256 * 1024 * 1024,
            min_free_bytes: 64 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024 * 1024,
            stop_after_saturated_samples: 8,
            gpu_busy_stop_percent: 97,
            require_disk_probe: true,
        }
    }
}

/// How the capture thread reacts to a full encode queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackpressurePolicy {
    /// Extra `yield_now` attempts before classifying `encode_backpressure`.
    /// Production should stay low so the capture poll stays responsive.
    pub encode_full_retries: u32,
    /// Encoder may wait this long on a full packet queue before mux drop.
    pub packet_send_timeout: Duration,
}

impl Default for BackpressurePolicy {
    fn default() -> Self {
        Self {
            encode_full_retries: 2,
            packet_send_timeout: Duration::from_millis(25),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub session_id: SessionId,
    pub queues: QueueConfig,
    pub backpressure: BackpressurePolicy,
    pub quality: QualityProfile,
    pub frame_rate_mode: FrameRateMode,
    /// Planned media duration. Live sessions may leave this `None`.
    pub duration: Option<Duration>,
    pub write_sidecar: bool,
    pub sidecar_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub limits: ResourceLimits,
}

impl SessionConfig {
    pub fn builder() -> SessionConfigBuilder {
        SessionConfigBuilder::default()
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionConfigBuilder {
    queues: Option<QueueConfig>,
    backpressure: Option<BackpressurePolicy>,
    quality: Option<QualityProfile>,
    frame_rate_mode: Option<FrameRateMode>,
    duration: Option<Duration>,
    write_sidecar: bool,
    sidecar_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    session_id: Option<SessionId>,
    limits: Option<ResourceLimits>,
}

impl SessionConfigBuilder {
    pub fn queues(mut self, queues: QueueConfig) -> Self {
        self.queues = Some(queues);
        self
    }

    pub fn backpressure(mut self, backpressure: BackpressurePolicy) -> Self {
        self.backpressure = Some(backpressure);
        self
    }

    pub fn quality(mut self, quality: QualityProfile) -> Self {
        self.quality = Some(quality);
        self
    }

    pub fn frame_rate_mode(mut self, mode: FrameRateMode) -> Self {
        self.frame_rate_mode = Some(mode);
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        if !duration.is_zero() {
            self.duration = Some(duration);
        }
        self
    }

    pub fn write_sidecar(mut self, enabled: bool) -> Self {
        self.write_sidecar = enabled;
        self
    }

    pub fn sidecar_dir(mut self, dir: PathBuf) -> Self {
        self.sidecar_dir = Some(dir);
        self
    }

    pub fn session_id(mut self, id: SessionId) -> Self {
        self.session_id = Some(id);
        self
    }

    pub fn output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = Some(dir);
        self
    }

    pub fn limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn build(self) -> SessionConfig {
        SessionConfig {
            session_id: self.session_id.unwrap_or_default(),
            queues: self.queues.unwrap_or_default(),
            backpressure: self.backpressure.unwrap_or_default(),
            quality: self.quality.unwrap_or_default(),
            frame_rate_mode: self
                .frame_rate_mode
                .unwrap_or(FrameRateMode::SourceAdaptive),
            duration: self.duration,
            write_sidecar: self.write_sidecar,
            sidecar_dir: self.sidecar_dir,
            output_dir: self.output_dir,
            limits: self.limits.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_source_adaptive() {
        let cfg = SessionConfig::builder().build();
        assert_eq!(cfg.frame_rate_mode, FrameRateMode::SourceAdaptive);
        assert!(cfg.duration.is_none());
        assert!(cfg.limits.max_pipeline_bytes > 0);
        assert!(cfg.limits.min_free_bytes > 0);
    }
}
