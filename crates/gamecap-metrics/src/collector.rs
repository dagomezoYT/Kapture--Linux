//! Atomic counters and interval reservoir observed by the metrics snapshot.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use gamecap_types::{DropReason, HostSample, MonotonicTimestamp, SafetyStopReason, SessionId};

use crate::summary::{DropCounters, SessionMetrics, TimingPercentiles};

const MAX_INTERVAL_SAMPLES: usize = 50_000;
const MAX_EVENTS: usize = 8_192;

#[derive(Debug)]
struct IntervalStore {
    samples_ns: Vec<u64>,
    last_ts: Option<MonotonicTimestamp>,
}

pub struct MetricsCollector {
    session_id: SessionId,
    frames_seen: AtomicU64,
    frames_captured: AtomicU64,
    frames_submitted: AtomicU64,
    frames_encoded: AtomicU64,
    duplicated: AtomicU64,
    late_frames: AtomicU64,
    gpu_copy_count: AtomicU64,
    zero_copy_frames: AtomicU64,
    audio_blocks: AtomicU64,
    audio_xruns: AtomicU64,
    bytes_written: AtomicU64,
    capture_queue_depth: AtomicU64,
    encoder_queue_depth: AtomicU64,
    packet_queue_depth: AtomicU64,
    drop_source_gap: AtomicU64,
    drop_capture_overrun: AtomicU64,
    drop_encode_backpressure: AtomicU64,
    drop_mux_backpressure: AtomicU64,
    drop_disk_backpressure: AtomicU64,
    drop_memory_limit: AtomicU64,
    pipeline_bytes: AtomicU64,
    disk_free_bytes: AtomicU64,
    disk_free_valid: AtomicU64,
    intervals: Mutex<IntervalStore>,
    start_ts: Mutex<Option<MonotonicTimestamp>>,
    end_ts: Mutex<Option<MonotonicTimestamp>>,
    host: Mutex<HostSample>,
    safety_stop: Mutex<Option<SafetyStopReason>>,
}

impl MetricsCollector {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            frames_seen: AtomicU64::new(0),
            frames_captured: AtomicU64::new(0),
            frames_submitted: AtomicU64::new(0),
            frames_encoded: AtomicU64::new(0),
            duplicated: AtomicU64::new(0),
            late_frames: AtomicU64::new(0),
            gpu_copy_count: AtomicU64::new(0),
            zero_copy_frames: AtomicU64::new(0),
            audio_blocks: AtomicU64::new(0),
            audio_xruns: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            capture_queue_depth: AtomicU64::new(0),
            encoder_queue_depth: AtomicU64::new(0),
            packet_queue_depth: AtomicU64::new(0),
            drop_source_gap: AtomicU64::new(0),
            drop_capture_overrun: AtomicU64::new(0),
            drop_encode_backpressure: AtomicU64::new(0),
            drop_mux_backpressure: AtomicU64::new(0),
            drop_disk_backpressure: AtomicU64::new(0),
            drop_memory_limit: AtomicU64::new(0),
            pipeline_bytes: AtomicU64::new(0),
            disk_free_bytes: AtomicU64::new(0),
            disk_free_valid: AtomicU64::new(0),
            intervals: Mutex::new(IntervalStore {
                samples_ns: Vec::new(),
                last_ts: None,
            }),
            start_ts: Mutex::new(None),
            end_ts: Mutex::new(None),
            host: Mutex::new(HostSample::default()),
            safety_stop: Mutex::new(None),
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn mark_start(&self, ts: MonotonicTimestamp) {
        if let Ok(mut guard) = self.start_ts.lock() {
            *guard = Some(ts);
        }
    }

    pub fn mark_end(&self, ts: MonotonicTimestamp) {
        if let Ok(mut guard) = self.end_ts.lock() {
            *guard = Some(ts);
        }
    }

    pub fn inc_frames_seen(&self) {
        self.frames_seen.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_frames_captured(&self) {
        self.frames_captured.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_frames_submitted(&self) {
        self.frames_submitted.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_frames_encoded(&self) {
        self.frames_encoded.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_duplicated(&self) {
        self.duplicated.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_audio_blocks(&self) {
        self.audio_blocks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_audio_xruns(&self) {
        self.audio_xruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes_written(&self, n: u64) {
        self.bytes_written.fetch_add(n, Ordering::Relaxed);
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }

    pub fn set_capture_queue_depth(&self, depth: u64) {
        self.capture_queue_depth.store(depth, Ordering::Relaxed);
    }

    pub fn set_encoder_queue_depth(&self, depth: u64) {
        self.encoder_queue_depth.store(depth, Ordering::Relaxed);
    }

    pub fn set_packet_queue_depth(&self, depth: u64) {
        self.packet_queue_depth.store(depth, Ordering::Relaxed);
    }

    pub fn record_zero_copy(&self, is_zero_copy: bool) {
        if is_zero_copy {
            self.zero_copy_frames.fetch_add(1, Ordering::Relaxed);
        } else {
            self.gpu_copy_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_drop(&self, reason: DropReason) {
        let counter = match reason {
            DropReason::SourceGap => &self.drop_source_gap,
            DropReason::CaptureOverrun => &self.drop_capture_overrun,
            DropReason::EncodeBackpressure => &self.drop_encode_backpressure,
            DropReason::MuxBackpressure => &self.drop_mux_backpressure,
            DropReason::DiskBackpressure => &self.drop_disk_backpressure,
            DropReason::MemoryLimit => &self.drop_memory_limit,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        let _ = MAX_EVENTS;
    }

    pub fn set_pipeline_bytes(&self, bytes: u64) {
        self.pipeline_bytes.store(bytes, Ordering::Relaxed);
    }

    pub fn set_disk_free_bytes(&self, bytes: Option<u64>) {
        match bytes {
            Some(v) => {
                self.disk_free_bytes.store(v, Ordering::Relaxed);
                self.disk_free_valid.store(1, Ordering::Relaxed);
            }
            None => {
                self.disk_free_valid.store(0, Ordering::Relaxed);
            }
        }
    }

    pub fn record_host_sample(&self, sample: HostSample) {
        if let Ok(mut guard) = self.host.lock() {
            *guard = sample;
        }
    }

    pub fn record_safety_stop(&self, reason: SafetyStopReason) {
        if let Ok(mut guard) = self.safety_stop.lock() {
            if guard.is_none() {
                *guard = Some(reason);
            }
        }
    }

    pub fn record_capture_interval(&self, ts: MonotonicTimestamp) {
        if let Ok(mut store) = self.intervals.lock() {
            if let Some(prev) = store.last_ts {
                let delta = ts.saturating_duration_since(prev).as_nanos();
                let delta = if delta > u128::from(u64::MAX) {
                    u64::MAX
                } else {
                    delta as u64
                };
                if store.samples_ns.len() < MAX_INTERVAL_SAMPLES {
                    store.samples_ns.push(delta);
                }
            }
            store.last_ts = Some(ts);
        }
    }

    pub fn snapshot(&self) -> SessionMetrics {
        let captured = self.frames_captured.load(Ordering::Relaxed);
        let encoded = self.frames_encoded.load(Ordering::Relaxed);
        let drops = DropCounters {
            source_gap: self.drop_source_gap.load(Ordering::Relaxed),
            capture_overrun: self.drop_capture_overrun.load(Ordering::Relaxed),
            encode_backpressure: self.drop_encode_backpressure.load(Ordering::Relaxed),
            mux_backpressure: self.drop_mux_backpressure.load(Ordering::Relaxed),
            disk_backpressure: self.drop_disk_backpressure.load(Ordering::Relaxed),
            memory_limit: self.drop_memory_limit.load(Ordering::Relaxed),
        };
        let timing = match self.intervals.lock() {
            Ok(store) => TimingPercentiles::from_samples(&store.samples_ns),
            Err(_) => TimingPercentiles::default(),
        };
        let start = self.start_ts.lock().ok().and_then(|g| *g);
        let end = self.end_ts.lock().ok().and_then(|g| *g);
        let duration = match (start, end) {
            (Some(s), Some(e)) => e.saturating_duration_since(s),
            _ => Duration::ZERO,
        };
        let zero_copy = self.zero_copy_frames.load(Ordering::Relaxed);
        let copies = self.gpu_copy_count.load(Ordering::Relaxed);
        let denom = zero_copy + copies;
        let zero_copy_ratio = if denom == 0 {
            0.0
        } else {
            (zero_copy as f64) * 100.0 / (denom as f64)
        };
        SessionMetrics {
            session_id: self.session_id,
            duration,
            frames_seen: self.frames_seen.load(Ordering::Relaxed),
            frames_captured: captured,
            frames_submitted: self.frames_submitted.load(Ordering::Relaxed),
            frames_encoded: encoded,
            duplicated: self.duplicated.load(Ordering::Relaxed),
            late_frames: self.late_frames.load(Ordering::Relaxed),
            drops,
            audio_blocks: self.audio_blocks.load(Ordering::Relaxed),
            audio_xruns: self.audio_xruns.load(Ordering::Relaxed),
            capture_queue_depth: self.capture_queue_depth.load(Ordering::Relaxed),
            encoder_queue_depth: self.encoder_queue_depth.load(Ordering::Relaxed),
            packet_queue_depth: self.packet_queue_depth.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            gpu_copy_count: copies,
            zero_copy_frames: zero_copy,
            zero_copy_ratio,
            timing,
            pipeline_bytes: self.pipeline_bytes.load(Ordering::Relaxed),
            disk_free_bytes: if self.disk_free_valid.load(Ordering::Relaxed) == 1 {
                Some(self.disk_free_bytes.load(Ordering::Relaxed))
            } else {
                None
            },
            host: self.host.lock().map(|g| g.clone()).unwrap_or_default(),
            safety_stop: self.safety_stop.lock().ok().and_then(|g| *g),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new(SessionId::new())
    }
}
