//! Resource governor: degrade or stop before the host wedges.
//!
//! Policy is pure. OS probes live in `gamecap-hardware` and are injected.

use gamecap_config::ResourceLimits;
use gamecap_types::{ResourceSnapshot, SafetyAction, SafetyStopReason};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaturationStreak {
    pub encoder: u32,
    pub gpu: u32,
    pub storage: u32,
    pub memory: u32,
    pub write_stall: u32,
}

pub fn evaluate(
    limits: &ResourceLimits,
    snap: &ResourceSnapshot,
    mut streak: SaturationStreak,
) -> (SafetyAction, SaturationStreak) {
    if let Some(free) = snap.disk_free_bytes {
        if free < limits.min_free_bytes {
            return (SafetyAction::Stop(SafetyStopReason::DiskSpace), streak);
        }
    }
    if limits.max_output_bytes > 0 && snap.bytes_written >= limits.max_output_bytes {
        return (SafetyAction::Stop(SafetyStopReason::OutputLimit), streak);
    }

    let encoder_sat =
        snap.video_queue_capacity > 0 && snap.video_queue_depth >= snap.video_queue_capacity;
    let packet_full =
        snap.packet_queue_capacity > 0 && snap.packet_queue_depth >= snap.packet_queue_capacity;
    let storage_sat = packet_full || snap.disk_backpressure_drops > 0;
    let gpu_sat = match snap.gpu_busy_percent {
        Some(pct) => pct >= f32::from(limits.gpu_busy_stop_percent),
        None => false,
    };
    let memory_sat = snap.pipeline_bytes >= limits.max_pipeline_bytes;

    streak.encoder = bump(streak.encoder, encoder_sat);
    streak.gpu = bump(streak.gpu, gpu_sat);
    streak.storage = bump(streak.storage, storage_sat);
    streak.memory = bump(streak.memory, memory_sat);
    streak.write_stall = bump(
        streak.write_stall,
        packet_full && snap.bytes_written_unchanged,
    );

    let stop_n = limits.stop_after_saturated_samples.max(1);
    if streak.write_stall >= stop_n {
        return (SafetyAction::Stop(SafetyStopReason::Watchdog), streak);
    }
    if streak.storage >= stop_n {
        return (
            SafetyAction::Stop(SafetyStopReason::StorageSaturation),
            streak,
        );
    }
    // A full encode queue already drops frames (bounded RAM). Stopping the
    // session here aborted real captures when software encode lagged.
    if streak.gpu >= stop_n {
        return (SafetyAction::Stop(SafetyStopReason::GpuSaturation), streak);
    }
    if streak.memory >= stop_n {
        return (SafetyAction::Stop(SafetyStopReason::Memory), streak);
    }

    if encoder_sat {
        return (
            SafetyAction::Degrade(SafetyStopReason::EncoderSaturation),
            streak,
        );
    }
    if gpu_sat {
        return (
            SafetyAction::Degrade(SafetyStopReason::GpuSaturation),
            streak,
        );
    }
    if storage_sat {
        return (
            SafetyAction::Degrade(SafetyStopReason::StorageSaturation),
            streak,
        );
    }
    if memory_high(snap.pipeline_bytes, limits.max_pipeline_bytes) {
        return (SafetyAction::Degrade(SafetyStopReason::Memory), streak);
    }

    (SafetyAction::Continue, streak)
}

fn bump(current: u32, active: bool) -> u32 {
    if active {
        current.saturating_add(1)
    } else {
        0
    }
}

fn memory_high(used: u64, limit: u64) -> bool {
    if limit == 0 {
        return false;
    }
    used.saturating_mul(10) >= limit.saturating_mul(9)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> ResourceLimits {
        ResourceLimits {
            max_pipeline_bytes: 1000,
            min_free_bytes: 100,
            max_output_bytes: 10_000,
            stop_after_saturated_samples: 3,
            gpu_busy_stop_percent: 97,
            require_disk_probe: true,
        }
    }

    #[test]
    fn low_disk_stops_immediately() {
        let snap = ResourceSnapshot {
            disk_free_bytes: Some(10),
            ..ResourceSnapshot::default()
        };
        let (action, _) = evaluate(&limits(), &snap, SaturationStreak::default());
        assert_eq!(action, SafetyAction::Stop(SafetyStopReason::DiskSpace));
    }

    #[test]
    fn output_cap_stops_writes() {
        let snap = ResourceSnapshot {
            disk_free_bytes: Some(10_000),
            bytes_written: 10_000,
            ..ResourceSnapshot::default()
        };
        let (action, _) = evaluate(&limits(), &snap, SaturationStreak::default());
        assert_eq!(action, SafetyAction::Stop(SafetyStopReason::OutputLimit));
    }

    #[test]
    fn encoder_full_degrades_without_stopping() {
        let snap = ResourceSnapshot {
            disk_free_bytes: Some(10_000),
            video_queue_depth: 8,
            video_queue_capacity: 8,
            ..ResourceSnapshot::default()
        };
        let mut streak = SaturationStreak::default();
        let (first, next) = evaluate(&limits(), &snap, streak);
        assert_eq!(
            first,
            SafetyAction::Degrade(SafetyStopReason::EncoderSaturation)
        );
        streak = next;
        let mut action = SafetyAction::Continue;
        for _ in 0..5 {
            let (a, s) = evaluate(&limits(), &snap, streak);
            action = a;
            streak = s;
        }
        assert_eq!(
            action,
            SafetyAction::Degrade(SafetyStopReason::EncoderSaturation)
        );
    }

    #[test]
    fn memory_streak_still_stops() {
        let snap = ResourceSnapshot {
            disk_free_bytes: Some(10_000),
            pipeline_bytes: 1000,
            ..ResourceSnapshot::default()
        };
        let mut streak = SaturationStreak::default();
        let mut action = SafetyAction::Continue;
        for _ in 0..5 {
            let (a, s) = evaluate(&limits(), &snap, streak);
            action = a;
            streak = s;
        }
        assert_eq!(action, SafetyAction::Stop(SafetyStopReason::Memory));
    }

    #[test]
    fn stalled_mux_is_watchdog_stop() {
        let snap = ResourceSnapshot {
            disk_free_bytes: Some(10_000),
            packet_queue_depth: 4,
            packet_queue_capacity: 4,
            bytes_written_unchanged: true,
            ..ResourceSnapshot::default()
        };
        let mut streak = SaturationStreak::default();
        let mut action = SafetyAction::Continue;
        for _ in 0..5 {
            let (a, s) = evaluate(&limits(), &snap, streak);
            action = a;
            streak = s;
        }
        assert_eq!(action, SafetyAction::Stop(SafetyStopReason::Watchdog));
    }
}
