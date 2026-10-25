//! Classified frame/audio drops. These must remain distinct in metrics.

use std::fmt;

/// Why a frame or packet was discarded. Names match the architecture document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropReason {
    SourceGap,
    CaptureOverrun,
    EncodeBackpressure,
    MuxBackpressure,
    DiskBackpressure,
    /// Pipeline RAM budget would be exceeded. Never grow unbounded buffers.
    MemoryLimit,
}

impl DropReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceGap => "source_gap",
            Self::CaptureOverrun => "capture_overrun",
            Self::EncodeBackpressure => "encode_backpressure",
            Self::MuxBackpressure => "mux_backpressure",
            Self::DiskBackpressure => "disk_backpressure",
            Self::MemoryLimit => "memory_limit",
        }
    }
}

impl fmt::Display for DropReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_wire_names() {
        assert_eq!(
            DropReason::EncodeBackpressure.as_str(),
            "encode_backpressure"
        );
        assert_eq!(DropReason::SourceGap.as_str(), "source_gap");
    }
}
