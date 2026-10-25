//! Opaque identifiers. No OS window/monitor types leak through these wrappers.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Identifier of a recording session. Unique per process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new() -> Self {
        Self(SESSION_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Display/monitor identity as reported by a platform crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MonitorId(u64);

impl MonitorId {
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

/// Window identity as reported by a platform crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

impl WindowId {
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

/// Monotonic packet sequence assigned by the encoder/mux boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PacketSeq(u64);

impl PacketSeq {
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}
