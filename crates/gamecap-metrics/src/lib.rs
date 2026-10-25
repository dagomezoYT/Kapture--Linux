//! Observability. Metrics never modify frames.

#![forbid(unsafe_code)]

mod collector;
mod summary;

pub use collector::MetricsCollector;
pub use summary::{DropCounters, SessionMetrics, SessionSummary, TimingPercentiles};
