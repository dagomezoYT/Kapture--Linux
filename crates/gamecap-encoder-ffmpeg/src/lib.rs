//! Delimited H.264 backends: FFmpeg (NVENC/VAAPI) and OpenH264 software.
//!
//! The FFmpeg path spawns a dedicated `ffmpeg` process (same isolation pattern
//! as GStreamer in capture). This crate is not imported by `gamecap-core`.

#![forbid(unsafe_code)]

mod ffmpeg_h264;
mod annexb;
mod openh264;

pub use annexb::AnnexBPassthroughEncoder;
pub use ffmpeg_h264::{FfmpegH264Encoder, HwKind};
pub use openh264::OpenH264Encoder;

/// Current implementation milestone for this crate.
pub const MILESTONE: &str = "M1-ffmpeg";
