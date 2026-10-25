//! Color/format policy. GPU conversion is a later milestone; CPU download is refused.

#![forbid(unsafe_code)]

use gamecap_types::{FrameStorage, PixelFormat, TypesError, VideoFormat, VideoFrame};

/// Native resolution: no scaling.
pub fn native_dimensions_ok(source: &VideoFormat, requested: &VideoFormat) -> bool {
    source.width == requested.width && source.height == requested.height
}

/// HDR must not be treated as SDR.
pub fn hdr_pipeline_valid(
    source: &VideoFrame,
    encoder_format: &VideoFormat,
) -> Result<(), TypesError> {
    if source.is_hdr() && !encoder_format.is_hdr() {
        return Err(TypesError::InvalidPixelFormat(
            "HDR source cannot feed an SDR encoder pipeline",
        ));
    }
    if encoder_format.is_hdr() && encoder_format.pixel_format.bit_depth() < 10 {
        return Err(TypesError::InvalidPixelFormat(
            "HDR pipeline requires 10-bit or greater",
        ));
    }
    Ok(())
}

/// Core-facing rule: never auto-download GPU frames.
pub fn refuse_cpu_download(storage: &FrameStorage) -> Result<(), TypesError> {
    if storage.is_gpu() {
        return Err(TypesError::InvalidPixelFormat(
            "automatic GPU-to-CPU download is forbidden; use an explicit fallback path",
        ));
    }
    Ok(())
}

pub fn same_format(a: PixelFormat, b: PixelFormat) -> bool {
    a == b
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use gamecap_types::{NativeHandle, NativeHandleKind};

    #[test]
    fn refuses_gpu_download() {
        let storage = FrameStorage::D3D11Texture {
            texture: NativeHandle {
                kind: NativeHandleKind::D3D11Texture,
                adapter_id: 0,
                resource_id: 1,
            },
            subresource: 0,
        };
        assert!(refuse_cpu_download(&storage).is_err());
    }

    #[test]
    fn hdr_not_silently_sdr() {
        let src_fmt = VideoFormat::hdr10(1920, 1080, PixelFormat::P010).unwrap();
        let sdr = VideoFormat::sdr_rec709(1920, 1080, PixelFormat::Bgra8).unwrap();
        let frame = VideoFrame {
            frame_id: 0,
            capture_ts: gamecap_types::MonotonicTimestamp::ZERO,
            source_ts: None,
            width: src_fmt.width,
            height: src_fmt.height,
            pixel_format: src_fmt.pixel_format,
            color_space: src_fmt.color_space,
            transfer: src_fmt.transfer,
            primaries: src_fmt.primaries,
            range: src_fmt.range,
            hdr_metadata: None,
            storage: FrameStorage::Cpu {
                bytes: std::sync::Arc::from(vec![0u8; 8]),
                stride: 8,
            },
        };
        assert!(hdr_pipeline_valid(&frame, &sdr).is_err());
        assert!(hdr_pipeline_valid(&frame, &src_fmt).is_ok());
    }
}
