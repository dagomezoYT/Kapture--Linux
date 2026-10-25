//! Bounded RAM for in-flight media. Capture never waits on disk to free this.

use std::sync::atomic::{AtomicU64, Ordering};

pub struct MemoryTracker {
    current: AtomicU64,
    limit: u64,
}

impl MemoryTracker {
    pub fn new(limit: u64) -> Self {
        Self {
            current: AtomicU64::new(0),
            limit: limit.max(1),
        }
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    pub fn try_reserve(&self, bytes: u64) -> bool {
        if bytes == 0 {
            return true;
        }
        loop {
            let now = self.current.load(Ordering::Relaxed);
            let Some(next) = now.checked_add(bytes) else {
                return false;
            };
            if next > self.limit {
                return false;
            }
            if self
                .current
                .compare_exchange_weak(now, next, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn release(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.current
            .fetch_sub(bytes.min(self.current()), Ordering::SeqCst);
    }
}

pub fn video_frame_bytes(frame: &gamecap_types::VideoFrame) -> u64 {
    match &frame.storage {
        gamecap_types::FrameStorage::Cpu { bytes, .. } => bytes.len() as u64,
        _ => frame
            .pixel_format
            .buffer_size(frame.width, frame.height)
            .map(|n| n as u64)
            .unwrap_or(0),
    }
}

pub fn audio_block_bytes(block: &gamecap_types::AudioBlock) -> u64 {
    (block.samples.len() * std::mem::size_of::<f32>()) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_exceed_limit() {
        let tracker = MemoryTracker::new(100);
        assert!(tracker.try_reserve(60));
        assert!(!tracker.try_reserve(50));
        tracker.release(60);
        assert!(tracker.try_reserve(50));
    }
}
