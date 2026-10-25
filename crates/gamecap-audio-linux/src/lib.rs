//! PipeWire audio backends. Milestone M1 uses a delimited GStreamer/Pulse adapter.

#![forbid(unsafe_code)]

mod gst_pipe;
mod pulse;

pub use pulse::PulseMonitorCapture;

pub const MILESTONE: &str = "M1";
