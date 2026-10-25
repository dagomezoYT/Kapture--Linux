//! Encoder ranking from capabilities. Hardware first; software last.

use gamecap_types::{EncoderCaps, EncoderVendor, FrameImportPath, FrameStorage, QualityProfile};

/// Request used to filter and rank encoder candidates.
#[derive(Clone, Debug)]
pub struct EncoderRequest {
    pub width: u32,
    pub height: u32,
    pub fps_num: Option<u32>,
    pub bit_depth: u8,
    pub hdr: bool,
    pub storage: FrameStorageKind,
    pub quality: QualityProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStorageKind {
    D3D11,
    DmaBuf,
    Vulkan,
    Cpu,
}

impl FrameStorageKind {
    pub fn from_storage(storage: &FrameStorage) -> Self {
        match storage {
            FrameStorage::D3D11Texture { .. } => Self::D3D11,
            FrameStorage::DmaBuf { .. } => Self::DmaBuf,
            FrameStorage::VulkanImage { .. } => Self::Vulkan,
            FrameStorage::Cpu { .. } => Self::Cpu,
        }
    }

    pub fn import_path(self) -> FrameImportPath {
        match self {
            Self::D3D11 => FrameImportPath::D3D11Shared,
            Self::DmaBuf => FrameImportPath::DmaBuf,
            Self::Vulkan => FrameImportPath::VulkanExport,
            Self::Cpu => FrameImportPath::CpuUpload,
        }
    }
}

pub fn is_compatible(caps: &EncoderCaps, request: &EncoderRequest) -> bool {
    if request.width > caps.max_width || request.height > caps.max_height {
        return false;
    }
    if request.hdr && !caps.supports_hdr {
        return false;
    }
    if !caps.bit_depths.contains(&request.bit_depth) {
        return false;
    }
    true
}

/// Rank compatible encoders: zero-copy, then hardware vendor, software last.
pub fn rank_encoders<'a>(
    candidates: &'a [EncoderCaps],
    request: &EncoderRequest,
) -> Vec<&'a EncoderCaps> {
    let mut compatible: Vec<&EncoderCaps> = candidates
        .iter()
        .filter(|caps| is_compatible(caps, request))
        .collect();

    let want = request.storage.import_path();
    compatible.sort_by(|a, b| {
        let a_zero = a.zero_copy_compatible && a.import_paths.contains(&want);
        let b_zero = b.zero_copy_compatible && b.import_paths.contains(&want);
        let zero = b_zero.cmp(&a_zero);
        if zero != std::cmp::Ordering::Equal {
            return zero;
        }
        let hw = b.vendor.is_hardware().cmp(&a.vendor.is_hardware());
        if hw != std::cmp::Ordering::Equal {
            return hw;
        }
        vendor_preference(a.vendor).cmp(&vendor_preference(b.vendor))
    });

    if matches!(request.quality, QualityProfile::Lossless) {
        compatible.retain(|c| {
            c.rc_modes.contains(&gamecap_types::RcMode::Lossless) || !c.vendor.is_hardware()
        });
    }
    compatible
}

fn vendor_preference(vendor: EncoderVendor) -> u8 {
    match vendor {
        EncoderVendor::Nvidia => 0,
        EncoderVendor::Intel => 1,
        EncoderVendor::Amd => 2,
        EncoderVendor::Test => 3,
        EncoderVendor::Software => 4,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use gamecap_types::{CodecId, RcMode};

    fn caps(vendor: EncoderVendor, zero: bool, import: FrameImportPath) -> EncoderCaps {
        EncoderCaps {
            vendor,
            name: vendor.as_str().to_string(),
            codecs: vec![CodecId::Hevc],
            bit_depths: vec![8, 10],
            max_width: 7680,
            max_height: 4320,
            max_fps: None,
            rc_modes: vec![RcMode::Cqp, RcMode::Lossless],
            import_paths: vec![import],
            supports_hdr: true,
            zero_copy_compatible: zero,
        }
    }

    #[test]
    fn software_is_last() {
        let list = vec![
            caps(EncoderVendor::Software, false, FrameImportPath::CpuUpload),
            caps(EncoderVendor::Nvidia, true, FrameImportPath::D3D11Shared),
        ];
        let request = EncoderRequest {
            width: 1920,
            height: 1080,
            fps_num: Some(144),
            bit_depth: 8,
            hdr: false,
            storage: FrameStorageKind::D3D11,
            quality: QualityProfile::HighQuality,
        };
        let ranked = rank_encoders(&list, &request);
        assert_eq!(ranked[0].vendor, EncoderVendor::Nvidia);
        assert_eq!(ranked[1].vendor, EncoderVendor::Software);
    }

    #[test]
    fn filters_resolution() {
        let mut tiny = caps(EncoderVendor::Intel, true, FrameImportPath::D3D11Shared);
        tiny.max_width = 1280;
        let request = EncoderRequest {
            width: 1920,
            height: 1080,
            fps_num: None,
            bit_depth: 8,
            hdr: false,
            storage: FrameStorageKind::D3D11,
            quality: QualityProfile::Performance,
        };
        assert!(rank_encoders(&[tiny], &request).is_empty());
    }
}
