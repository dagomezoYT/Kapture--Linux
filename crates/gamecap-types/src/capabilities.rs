//! Capability models. The UI must only offer combinations present here.

use crate::audio::AudioFormat;
use crate::ids::MonitorId;
use crate::packet::CodecId;
use crate::rational::Rational;
use crate::video::PixelFormat;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureBackendKind {
    WindowsGraphicsCapture,
    DxgiDuplication,
    PipeWirePortal,
    X11,
    Fake,
}

impl CaptureBackendKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WindowsGraphicsCapture => "wgc",
            Self::DxgiDuplication => "dxgi",
            Self::PipeWirePortal => "pipewire-portal",
            Self::X11 => "x11",
            Self::Fake => "fake",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    pub id: MonitorId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh: Option<Rational>,
    pub hdr_capable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureCaps {
    pub backend: CaptureBackendKind,
    pub monitors: Vec<DisplayInfo>,
    pub supports_window_capture: bool,
    pub supports_hdr: bool,
    pub pixel_formats: Vec<PixelFormat>,
    pub max_fps: Option<Rational>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EncoderVendor {
    Nvidia,
    Intel,
    Amd,
    Software,
    Test,
}

impl EncoderVendor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nvidia => "nvidia",
            Self::Intel => "intel",
            Self::Amd => "amd",
            Self::Software => "software",
            Self::Test => "test",
        }
    }

    pub const fn is_hardware(self) -> bool {
        matches!(self, Self::Nvidia | Self::Intel | Self::Amd)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RcMode {
    Cbr,
    Vbr,
    Cqp,
    Icq,
    Lossless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameImportPath {
    D3D11Shared,
    DmaBuf,
    VulkanExport,
    CpuUpload,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EncoderCaps {
    pub vendor: EncoderVendor,
    pub name: String,
    pub codecs: Vec<CodecId>,
    pub bit_depths: Vec<u8>,
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: Option<Rational>,
    pub rc_modes: Vec<RcMode>,
    pub import_paths: Vec<FrameImportPath>,
    pub supports_hdr: bool,
    pub zero_copy_compatible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuAdapterInfo {
    pub id: u64,
    pub name: String,
    pub vendor: EncoderVendor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_input: bool,
    pub default_format: Option<AudioFormat>,
}

/// Snapshot of what this host can actually do. Built by probing, never guessed.
#[derive(Clone, Debug, PartialEq)]
pub struct CapabilityReport {
    pub os: String,
    pub session_type: String,
    pub displays: Vec<DisplayInfo>,
    pub refresh_rates: Vec<Rational>,
    pub hdr_capable: bool,
    pub capture_backends: Vec<CaptureBackendKind>,
    pub gpu_adapters: Vec<GpuAdapterInfo>,
    pub frame_import_paths: Vec<FrameImportPath>,
    pub encoders: Vec<EncoderCaps>,
    pub audio_inputs: Vec<AudioDeviceInfo>,
    pub audio_outputs: Vec<AudioDeviceInfo>,
    pub storage_throughput_estimate_bps: Option<u64>,
}

impl CapabilityReport {
    pub fn empty(os: impl Into<String>) -> Self {
        Self {
            os: os.into(),
            session_type: "unprobed".to_string(),
            displays: Vec::new(),
            refresh_rates: Vec::new(),
            hdr_capable: false,
            capture_backends: Vec::new(),
            gpu_adapters: Vec::new(),
            frame_import_paths: Vec::new(),
            encoders: Vec::new(),
            audio_inputs: Vec::new(),
            audio_outputs: Vec::new(),
            storage_throughput_estimate_bps: None,
        }
    }
}
