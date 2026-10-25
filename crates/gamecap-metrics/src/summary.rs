//! Snapshots and human-readable session summaries.

use std::fmt;
use std::time::Duration;

use gamecap_types::{json, HostSample, SafetyStopReason, SessionId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DropCounters {
    pub source_gap: u64,
    pub capture_overrun: u64,
    pub encode_backpressure: u64,
    pub mux_backpressure: u64,
    pub disk_backpressure: u64,
    pub memory_limit: u64,
}

impl DropCounters {
    pub fn total(&self) -> u64 {
        self.source_gap
            .saturating_add(self.capture_overrun)
            .saturating_add(self.encode_backpressure)
            .saturating_add(self.mux_backpressure)
            .saturating_add(self.disk_backpressure)
            .saturating_add(self.memory_limit)
    }

    /// Drops caused by the recorder core (queues / RAM), not by a missing source frame.
    pub fn core_caused(&self) -> u64 {
        self.capture_overrun
            .saturating_add(self.encode_backpressure)
            .saturating_add(self.mux_backpressure)
            .saturating_add(self.disk_backpressure)
            .saturating_add(self.memory_limit)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimingPercentiles {
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub samples: u64,
}

impl TimingPercentiles {
    pub fn from_samples(samples: &[u64]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        Self {
            p50_ns: percentile(&sorted, 50),
            p95_ns: percentile(&sorted, 95),
            p99_ns: percentile(&sorted, 99),
            samples: n as u64,
        }
    }

    pub fn p50_ms(&self) -> f64 {
        self.p50_ns as f64 / 1_000_000.0
    }

    pub fn p95_ms(&self) -> f64 {
        self.p95_ns as f64 / 1_000_000.0
    }

    pub fn p99_ms(&self) -> f64 {
        self.p99_ns as f64 / 1_000_000.0
    }
}

fn percentile(sorted: &[u64], pct: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let max_index = sorted.len() - 1;
    let idx = (u64::from(pct) * max_index as u64) / 100;
    sorted[idx as usize]
}

#[derive(Clone, Debug)]
pub struct SessionMetrics {
    pub session_id: SessionId,
    pub duration: Duration,
    pub frames_seen: u64,
    pub frames_captured: u64,
    pub frames_submitted: u64,
    pub frames_encoded: u64,
    pub duplicated: u64,
    pub late_frames: u64,
    pub drops: DropCounters,
    pub audio_blocks: u64,
    pub audio_xruns: u64,
    pub capture_queue_depth: u64,
    pub encoder_queue_depth: u64,
    pub packet_queue_depth: u64,
    pub bytes_written: u64,
    pub gpu_copy_count: u64,
    pub zero_copy_frames: u64,
    pub zero_copy_ratio: f64,
    pub timing: TimingPercentiles,
    pub pipeline_bytes: u64,
    pub disk_free_bytes: Option<u64>,
    pub host: HostSample,
    pub safety_stop: Option<SafetyStopReason>,
}

impl SessionMetrics {
    pub fn capture_efficiency(&self) -> f64 {
        if self.frames_seen == 0 {
            100.0
        } else {
            (self.frames_captured as f64) * 100.0 / (self.frames_seen as f64)
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub metrics: SessionMetrics,
}

impl SessionSummary {
    pub fn new(metrics: SessionMetrics) -> Self {
        Self { metrics }
    }

    pub fn to_json(&self) -> String {
        let m = &self.metrics;
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"session_id\": {},\n", m.session_id.as_raw()));
        s.push_str(&format!(
            "  \"duration_ns\": {},\n",
            saturate_duration_ns(m.duration)
        ));
        s.push_str(&format!("  \"frames_seen\": {},\n", m.frames_seen));
        s.push_str(&format!("  \"frames_captured\": {},\n", m.frames_captured));
        s.push_str(&format!(
            "  \"frames_submitted\": {},\n",
            m.frames_submitted
        ));
        s.push_str(&format!("  \"frames_encoded\": {},\n", m.frames_encoded));
        s.push_str(&format!("  \"duplicated\": {},\n", m.duplicated));
        s.push_str(&format!("  \"dropped_total\": {},\n", m.drops.total()));
        s.push_str(&format!(
            "  \"dropped_core_caused\": {},\n",
            m.drops.core_caused()
        ));
        s.push_str("  \"drops\": {\n");
        s.push_str(&format!("    \"source_gap\": {},\n", m.drops.source_gap));
        s.push_str(&format!(
            "    \"capture_overrun\": {},\n",
            m.drops.capture_overrun
        ));
        s.push_str(&format!(
            "    \"encode_backpressure\": {},\n",
            m.drops.encode_backpressure
        ));
        s.push_str(&format!(
            "    \"mux_backpressure\": {},\n",
            m.drops.mux_backpressure
        ));
        s.push_str(&format!(
            "    \"disk_backpressure\": {},\n",
            m.drops.disk_backpressure
        ));
        s.push_str(&format!("    \"memory_limit\": {}\n", m.drops.memory_limit));
        s.push_str("  },\n");
        s.push_str(&format!("  \"audio_blocks\": {},\n", m.audio_blocks));
        s.push_str(&format!("  \"audio_xruns\": {},\n", m.audio_xruns));
        s.push_str(&format!(
            "  \"frame_interval_p50_ns\": {},\n",
            m.timing.p50_ns
        ));
        s.push_str(&format!(
            "  \"frame_interval_p95_ns\": {},\n",
            m.timing.p95_ns
        ));
        s.push_str(&format!(
            "  \"frame_interval_p99_ns\": {},\n",
            m.timing.p99_ns
        ));
        s.push_str(&format!("  \"zero_copy_ratio\": {},\n", m.zero_copy_ratio));
        s.push_str(&format!("  \"pipeline_bytes\": {},\n", m.pipeline_bytes));
        s.push_str(&format!(
            "  \"disk_free_bytes\": {},\n",
            option_u64_json(m.disk_free_bytes)
        ));
        s.push_str(&format!(
            "  \"cpu_load_1m\": {},\n",
            option_f32_json(m.host.cpu_load_1m)
        ));
        s.push_str(&format!(
            "  \"gpu_busy_percent\": {},\n",
            option_f32_json(m.host.gpu_busy_percent)
        ));
        s.push_str(&format!(
            "  \"temperatures\": [{}],\n",
            m.host
                .temperatures_c
                .iter()
                .map(|t| format!(
                    "{{\"sensor\":{},\"celsius\":{}}}",
                    json::string_value(&t.sensor),
                    t.celsius
                ))
                .collect::<Vec<_>>()
                .join(",")
        ));
        s.push_str(&format!(
            "  \"elevated_privileges\": {},\n",
            m.host.elevated_privileges
        ));
        s.push_str(&format!(
            "  \"safety_stop\": {},\n",
            match m.safety_stop {
                Some(r) => json::string_value(r.as_str()),
                None => "null".to_string(),
            }
        ));
        s.push_str(&format!(
            "  \"capture_efficiency\": {}\n",
            m.capture_efficiency()
        ));
        s.push('}');
        let _ = json::escape;
        s
    }
}

impl fmt::Display for SessionSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = &self.metrics;
        writeln!(f, "Duration: {}", format_duration(m.duration))?;
        writeln!(f, "Captured: {}", m.frames_captured)?;
        writeln!(f, "Encoded: {}", m.frames_encoded)?;
        writeln!(f, "Dropped: {}", m.drops.total())?;
        writeln!(f, "  source_gap: {}", m.drops.source_gap)?;
        writeln!(f, "  capture_overrun: {}", m.drops.capture_overrun)?;
        writeln!(f, "  encode_backpressure: {}", m.drops.encode_backpressure)?;
        writeln!(f, "  mux_backpressure: {}", m.drops.mux_backpressure)?;
        writeln!(f, "  disk_backpressure: {}", m.drops.disk_backpressure)?;
        writeln!(f, "  memory_limit: {}", m.drops.memory_limit)?;
        writeln!(f, "Duplicated: {}", m.duplicated)?;
        writeln!(f, "Capture efficiency: {:.3}%", m.capture_efficiency())?;
        writeln!(
            f,
            "Frame interval p50/p95/p99: {:.2} / {:.2} / {:.2} ms",
            m.timing.p50_ms(),
            m.timing.p95_ms(),
            m.timing.p99_ms()
        )?;
        writeln!(f, "Zero-copy ratio: {:.3}%", m.zero_copy_ratio)?;
        writeln!(f, "Audio blocks: {}", m.audio_blocks)?;
        writeln!(f, "Audio xruns: {}", m.audio_xruns)?;
        if let Some(free) = m.disk_free_bytes {
            writeln!(f, "Disk free: {free} bytes")?;
        }
        if let Some(load) = m.host.cpu_load_1m {
            writeln!(f, "CPU load 1m: {load:.2}")?;
        }
        if let Some(gpu) = m.host.gpu_busy_percent {
            writeln!(f, "GPU busy: {gpu:.1}%")?;
        }
        for temp in &m.host.temperatures_c {
            writeln!(f, "Temp {}: {:.1} C", temp.sensor, temp.celsius)?;
        }
        if let Some(reason) = m.safety_stop {
            writeln!(f, "Safety stop: {reason}")?;
        }
        Ok(())
    }
}

fn format_duration(duration: Duration) -> String {
    let total_ms = duration.as_millis();
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

fn option_u64_json(value: Option<u64>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "null".to_string(),
    }
}

fn option_f32_json(value: Option<f32>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "null".to_string(),
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
mod tests {
    use super::*;

    #[test]
    fn percentile_of_uniform() {
        let samples: Vec<u64> = (1..=100).collect();
        let t = TimingPercentiles::from_samples(&samples);
        assert_eq!(t.p50_ns, 50);
        assert_eq!(t.p99_ns, 99);
        assert_eq!(t.samples, 100);
    }

    #[test]
    fn core_caused_excludes_source_gap() {
        let drops = DropCounters {
            source_gap: 10,
            encode_backpressure: 2,
            ..DropCounters::default()
        };
        assert_eq!(drops.total(), 12);
        assert_eq!(drops.core_caused(), 2);
    }
}
