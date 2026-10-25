//! Unified video frame and opaque GPU storage.
//!
//! The core must never convert a GPU texture into `Vec<u8>` automatically.

use std::sync::Arc;

use crate::color::{ColorPrimaries, ColorRange, ColorSpace, HdrMetadata, TransferFunction};
use crate::timestamp::{MonotonicTimestamp, SourceTimestamp};
use crate::video::PixelFormat;

/// Opaque identifier for a native GPU object owned by a platform crate.
///
/// `gamecap-core` may pass this through encoder selection but must not
/// dereference it. Platform crates map `resource_id` to D3D11/Vulkan/etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeHandle {
    pub kind: NativeHandleKind,
    pub adapter_id: u64,
    pub resource_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NativeHandleKind {
    D3D11Texture,
    VulkanImage,
    VulkanMemory,
    VaSurface,
}

/// DMA-BUF plane without `RawFd`. The linux capture crate owns the fd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DmaBufPlane {
    pub fd_id: u64,
    pub stride: u32,
    pub offset: u32,
    pub size: u32,
}

/// Where pixels live. CPU is fallback / synthetic only.
#[derive(Clone, Debug)]
pub enum FrameStorage {
    D3D11Texture {
        texture: NativeHandle,
        subresource: u32,
    },
    DmaBuf {
        planes: Vec<DmaBufPlane>,
        modifier: u64,
    },
    VulkanImage {
        image: NativeHandle,
        memory: NativeHandle,
    },
    Cpu {
        bytes: Arc<[u8]>,
        stride: usize,
    },
}

impl FrameStorage {
    pub fn is_gpu(&self) -> bool {
        !matches!(self, Self::Cpu { .. })
    }

    pub fn cpu_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Cpu { bytes, .. } => Some(bytes.as_ref()),
            _ => None,
        }
    }
}

/// Unified captured frame. Timestamps must already be in the master clock domain.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub frame_id: u64,
    pub capture_ts: MonotonicTimestamp,
    pub source_ts: Option<SourceTimestamp>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub color_space: ColorSpace,
    pub transfer: TransferFunction,
    pub primaries: ColorPrimaries,
    pub range: ColorRange,
    pub hdr_metadata: Option<HdrMetadata>,
    pub storage: FrameStorage,
}

impl VideoFrame {
    pub fn is_hdr(&self) -> bool {
        self.transfer.is_hdr() || self.hdr_metadata.is_some()
    }

    /// Identity header written by synthetic backends: little-endian frame_id.
    pub fn identity_frame_id(&self) -> Option<u64> {
        let bytes = self.storage.cpu_bytes()?;
        if bytes.len() < 8 {
            return None;
        }
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&bytes[..8]);
        Some(u64::from_le_bytes(raw))
    }
}

/// Borrowed encoder input. Does not download GPU resources.
#[derive(Clone, Copy, Debug)]
pub struct GpuFrameRef<'a> {
    frame: &'a VideoFrame,
}

impl<'a> GpuFrameRef<'a> {
    pub fn new(frame: &'a VideoFrame) -> Self {
        Self { frame }
    }

    pub fn frame(self) -> &'a VideoFrame {
        self.frame
    }

    pub fn storage(self) -> &'a FrameStorage {
        &self.frame.storage
    }

    /// Available only when storage is already CPU. This is not a download API.
    pub fn existing_cpu_bytes(self) -> Option<&'a [u8]> {
        self.frame.storage.cpu_bytes()
    }
}

impl<'a> From<&'a VideoFrame> for GpuFrameRef<'a> {
    fn from(frame: &'a VideoFrame) -> Self {
        Self::new(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_storage_has_no_cpu_bytes() {
        let storage = FrameStorage::D3D11Texture {
            texture: NativeHandle {
                kind: NativeHandleKind::D3D11Texture,
                adapter_id: 0,
                resource_id: 1,
            },
            subresource: 0,
        };
        assert!(storage.is_gpu());
        assert!(storage.cpu_bytes().is_none());
    }
}
