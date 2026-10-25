//! Session master clock. Platform backends convert QPC/PipeWire into this domain.

use std::time::Instant;

use gamecap_types::MonotonicTimestamp;

#[derive(Clone, Debug)]
pub struct MasterClock {
    origin: Instant,
}

impl MasterClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    pub fn origin(&self) -> Instant {
        self.origin
    }

    pub fn now(&self) -> MonotonicTimestamp {
        MonotonicTimestamp::from_duration(self.origin.elapsed())
    }
}

impl Default for MasterClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn monotonic_increases() {
        let clock = MasterClock::new();
        let a = clock.now();
        thread::sleep(Duration::from_millis(1));
        let b = clock.now();
        assert!(b >= a);
    }
}
