//! Video format, capture request enums, and quality profiles as policies.

use crate::color::{ColorPrimaries, ColorRange, ColorSpace, TransferFunction};
use crate::error::TypesError;
use crate::ids::{MonitorId, WindowId};
use crate::rational::Rational;

/// Pixel packing. Bit depth is a property of the format, never guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
    Nv12,
    P010,
    Rgba16Float,
}

impl PixelFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bgra8 => "bgra8",
            Self::Rgba8 => "rgba8",
            Self::Nv12 => "nv12",
            Self::P010 => "p010",
            Self::Rgba16Float => "rgba16f",
        }
    }

    pub const fn bit_depth(self) -> u8 {
        match self {
            Self::Bgra8 | Self::Rgba8 | Self::Nv12 => 8,
            Self::P010 => 10,
            Self::Rgba16Float => 16,
        }
    }

    pub const fn is_yuv(self) -> bool {
        matches!(self, Self::Nv12 | Self::P010)
    }

    /// Bytes per pixel for packed RGB formats. `None` for planar YUV.
    pub const fn packed_bytes_per_pixel(self) -> Option<usize> {
        match self {
            Self::Bgra8 | Self::Rgba8 => Some(4),
            Self::Rgba16Float => Some(8),
            Self::Nv12 | Self::P010 => None,
        }
    }

    pub fn stride_bytes(self, width: u32) -> Result<usize, TypesError> {
        if width == 0 {
            return Err(TypesError::InvalidDimension { width, height: 0 });
        }
        match self {
            Self::Bgra8 | Self::Rgba8 => Ok(width as usize * 4),
            Self::Rgba16Float => Ok(width as usize * 8),
            Self::Nv12 => Ok(width as usize),
            Self::P010 => Ok(width as usize * 2),
        }
    }

    pub fn buffer_size(self, width: u32, height: u32) -> Result<usize, TypesError> {
        if width == 0 || height == 0 {
            return Err(TypesError::InvalidDimension { width, height });
        }
        match self {
            Self::Bgra8 | Self::Rgba8 => Ok(width as usize * height as usize * 4),
            Self::Rgba16Float => Ok(width as usize * height as usize * 8),
            Self::Nv12 => {
                let y = width as usize * height as usize;
                Ok(y + y / 2)
            }
            Self::P010 => {
                let y = width as usize * height as usize * 2;
                Ok(y + y / 2)
            }
        }
    }
}

/// Complete video signal description. SDR and HDR are explicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFormat {
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub color_space: ColorSpace,
    pub transfer: TransferFunction,
    pub primaries: ColorPrimaries,
    pub range: ColorRange,
    pub frame_rate: Option<Rational>,
}

impl VideoFormat {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.width == 0 || self.height == 0 {
            return Err(TypesError::InvalidDimension {
                width: self.width,
                height: self.height,
            });
        }
        Ok(())
    }

    pub fn sdr_rec709(
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
    ) -> Result<Self, TypesError> {
        let fmt = Self {
            width,
            height,
            pixel_format,
            color_space: ColorSpace::Bt709,
            transfer: TransferFunction::Bt709,
            primaries: ColorPrimaries::Bt709,
            range: ColorRange::Full,
            frame_rate: None,
        };
        fmt.validate()?;
        Ok(fmt)
    }

    pub fn hdr10(width: u32, height: u32, pixel_format: PixelFormat) -> Result<Self, TypesError> {
        if pixel_format.bit_depth() < 10 {
            return Err(TypesError::InvalidPixelFormat(
                "HDR10 requires a 10-bit or greater pixel format",
            ));
        }
        let fmt = Self {
            width,
            height,
            pixel_format,
            color_space: ColorSpace::Bt2020,
            transfer: TransferFunction::PqSt2084,
            primaries: ColorPrimaries::Bt2020,
            range: ColorRange::Limited,
            frame_rate: None,
        };
        fmt.validate()?;
        Ok(fmt)
    }

    pub fn is_hdr(&self) -> bool {
        self.transfer.is_hdr()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureTarget {
    Monitor {
        id: MonitorId,
    },
    Window {
        id: WindowId,
    },
    Region {
        monitor: MonitorId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// Used by synthetic backends in tests. Not a real display.
    Synthetic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorMode {
    Exclude,
    Embedded,
    Metadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HdrPolicy {
    Auto,
    ForceSdr,
    RequireHdr,
}

/// Frame-rate policy. The core must not invent content in Source/Adaptive mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameRateMode {
    SourceAdaptive,
    DisplaySync { refresh: Rational },
    FixedCfr { fps: Rational },
    Custom { fps: Rational },
}

impl FrameRateMode {
    pub fn target_fps(self) -> Option<Rational> {
        match self {
            Self::SourceAdaptive => None,
            Self::DisplaySync { refresh } => Some(refresh),
            Self::FixedCfr { fps } | Self::Custom { fps } => Some(fps),
        }
    }
}

/// Quality profile is a policy over capabilities, not a hardcoded bitrate table.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum QualityProfile {
    Performance,
    #[default]
    HighQuality,
    NearLossless,
    Lossless,
    Advanced(AdvancedEncodeSettings),
}

/// Expert controls. Values are still validated against EncoderCaps at selection time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvancedEncodeSettings {
    pub codec: Option<crate::packet::CodecId>,
    pub preset: Option<String>,
    pub gop_frames: Option<u32>,
    pub b_frames: Option<u32>,
    pub bit_depth: Option<u8>,
    pub bitrate_kbps: Option<u32>,
    pub qp: Option<u32>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hdr10_rejects_8bit() {
        let err = VideoFormat::hdr10(1920, 1080, PixelFormat::Bgra8).unwrap_err();
        assert_eq!(
            err,
            TypesError::InvalidPixelFormat("HDR10 requires a 10-bit or greater pixel format")
        );
    }

    #[test]
    fn hdr10_p010_is_hdr() {
        let fmt = VideoFormat::hdr10(1920, 1080, PixelFormat::P010).unwrap();
        assert!(fmt.is_hdr());
        assert_eq!(fmt.pixel_format.bit_depth(), 10);
    }

    #[test]
    fn sdr_is_not_hdr() {
        let fmt = VideoFormat::sdr_rec709(1920, 1080, PixelFormat::Bgra8).unwrap();
        assert!(!fmt.is_hdr());
    }
}
