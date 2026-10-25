//! Monotonic timestamps in a single clock domain.

use std::time::Duration;

use crate::rational::Rational;

/// Nanoseconds on the session master clock.
///
/// Origin is arbitrary (typically session start). Comparisons are only valid
/// within the same session. Platform backends convert QPC / PipeWire times
/// into this domain before the frame enters `gamecap-core`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicTimestamp {
    nanos: u64,
}

impl MonotonicTimestamp {
    pub const ZERO: Self = Self { nanos: 0 };

    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    pub const fn as_nanos(self) -> u64 {
        self.nanos
    }

    pub fn from_duration(duration: Duration) -> Self {
        Self {
            nanos: saturate_u128_to_u64(duration.as_nanos()),
        }
    }

    pub fn saturating_add(self, duration: Duration) -> Self {
        let add = saturate_u128_to_u64(duration.as_nanos());
        Self {
            nanos: self.nanos.saturating_add(add),
        }
    }

    pub fn saturating_add_nanos(self, nanos: u64) -> Self {
        Self {
            nanos: self.nanos.saturating_add(nanos),
        }
    }

    pub fn duration_since(self, earlier: Self) -> Option<Duration> {
        self.nanos
            .checked_sub(earlier.nanos)
            .map(Duration::from_nanos)
    }

    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.nanos.saturating_sub(earlier.nanos))
    }

    pub fn as_duration(self) -> Duration {
        Duration::from_nanos(self.nanos)
    }
}

/// Raw timestamp as reported by the capture source, before or after conversion.
/// Kept for diagnostics; muxing uses [`MonotonicTimestamp`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceTimestamp {
    nanos: u64,
}

impl SourceTimestamp {
    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    pub const fn as_nanos(self) -> u64 {
        self.nanos
    }

    pub fn from_master(ts: MonotonicTimestamp) -> Self {
        Self {
            nanos: ts.as_nanos(),
        }
    }
}

/// Media PTS for `frame_id` at `fps` using integer arithmetic.
///
/// `t_ns = frame_id * 1_000_000_000 * den / num`
pub fn media_pts(frame_id: u64, fps: Rational) -> MonotonicTimestamp {
    if fps.num() == 0 {
        return MonotonicTimestamp::ZERO;
    }
    let nanos = u128::from(frame_id)
        .saturating_mul(1_000_000_000)
        .saturating_mul(u128::from(fps.den()))
        / u128::from(fps.num());
    MonotonicTimestamp::from_nanos(saturate_u128_to_u64(nanos))
}

/// Media PTS for audio sample position `sample_index` at `sample_rate_hz`.
pub fn media_pts_samples(sample_index: u64, sample_rate_hz: u32) -> MonotonicTimestamp {
    if sample_rate_hz == 0 {
        return MonotonicTimestamp::ZERO;
    }
    let nanos = u128::from(sample_index).saturating_mul(1_000_000_000) / u128::from(sample_rate_hz);
    MonotonicTimestamp::from_nanos(saturate_u128_to_u64(nanos))
}

fn saturate_u128_to_u64(value: u128) -> u64 {
    if value > u128::from(u64::MAX) {
        u64::MAX
    } else {
        value as u64
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::rational::Rational;

    #[test]
    fn ten_seconds_at_144_fps_is_exact() {
        let fps = Rational::new(144, 1).unwrap();
        let ts = media_pts(1_440, fps);
        assert_eq!(ts.as_nanos(), 10_000_000_000);
        let first = media_pts(1, fps);
        assert_eq!(first.as_nanos(), 1_000_000_000 / 144);
    }

    #[test]
    fn audio_48k_ten_seconds() {
        let ts = media_pts_samples(480_000, 48_000);
        assert_eq!(ts.as_nanos(), 10_000_000_000);
    }

    #[test]
    fn ordering() {
        let a = MonotonicTimestamp::from_nanos(1);
        let b = MonotonicTimestamp::from_nanos(2);
        assert!(a < b);
        assert_eq!(b.saturating_duration_since(a), Duration::from_nanos(1));
    }
}
