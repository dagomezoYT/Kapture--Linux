//! Linux capture backends: xdg-desktop-portal + PipeWire via a delimited GStreamer adapter.

#![forbid(unsafe_code)]

mod av1_obu;
mod gst_pipe;
mod h264_au;
mod portal;
mod refresh;

pub use gst_pipe::{gst_nvh264enc_available, gst_vaav1enc_available};
pub use portal::PortalPipeWireCapture;
pub use refresh::{
    detect_display_mode, detect_display_refresh_hz, refresh_hz_for_request, DisplayMode,
};

/// Current implementation milestone for this crate.
pub const MILESTONE: &str = "M1";
