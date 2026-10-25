//! Explicit SDR/HDR color metadata. HDR is never implied by bit depth alone.

use std::fmt;

/// Color encoding matrix / space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    Bt601,
    Bt709,
    Bt2020,
    Unknown,
}

impl ColorSpace {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bt601 => "bt601",
            Self::Bt709 => "bt709",
            Self::Bt2020 => "bt2020",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ColorSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Opto-electronic transfer function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferFunction {
    Srgb,
    Bt709,
    PqSt2084,
    Hlg,
    Linear,
    Unknown,
}

impl TransferFunction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Srgb => "srgb",
            Self::Bt709 => "bt709",
            Self::PqSt2084 => "pq-st2084",
            Self::Hlg => "hlg",
            Self::Linear => "linear",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_hdr(self) -> bool {
        matches!(self, Self::PqSt2084 | Self::Hlg)
    }
}

impl fmt::Display for TransferFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorPrimaries {
    Bt709,
    Bt2020,
    DciP3,
    Unknown,
}

impl ColorPrimaries {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bt709 => "bt709",
            Self::Bt2020 => "bt2020",
            Self::DciP3 => "dci-p3",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ColorPrimaries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorRange {
    Limited,
    Full,
}

impl ColorRange {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Limited => "limited",
            Self::Full => "full",
        }
    }
}

impl fmt::Display for ColorRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// HDR10 static metadata. Absence means SDR unless transfer says otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct HdrMetadata {
    pub max_cll: Option<u32>,
    pub max_fall: Option<u32>,
    pub mastering: Option<MasteringDisplay>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MasteringDisplay {
    pub red: [u16; 2],
    pub green: [u16; 2],
    pub blue: [u16; 2],
    pub white_point: [u16; 2],
    pub max_luminance: u32,
    pub min_luminance: u32,
}

impl HdrMetadata {
    pub fn is_present(&self) -> bool {
        self.max_cll.is_some() || self.max_fall.is_some() || self.mastering.is_some()
    }
}
