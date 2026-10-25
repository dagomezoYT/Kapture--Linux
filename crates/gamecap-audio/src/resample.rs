//! Identity resample. Real SRC lands when live devices appear.
//!
//! Drift correction must record the ratio; it must never insert/delete samples
//! silently.

use gamecap_types::{AudioBlock, AudioError, INTERNAL_SAMPLE_RATE_HZ};

/// Resample a block toward the internal 48 kHz rate.
///
/// M0 only accepts already-48 kHz input (cheap Arc clone). A different rate is
/// an explicit error so we never hide a conversion.
pub fn resample_to_internal(block: &AudioBlock) -> Result<AudioBlock, AudioError> {
    if block.format.sample_rate.hz() == INTERNAL_SAMPLE_RATE_HZ {
        return Ok(block.clone());
    }
    Err(AudioError::UnsupportedCombination(
        gamecap_types::UnsupportedCombination::new(
            "resample",
            format!(
                "M0 identity resampler cannot convert {} Hz to {INTERNAL_SAMPLE_RATE_HZ} Hz",
                block.format.sample_rate.hz()
            ),
        ),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use gamecap_types::{
        AudioFormat, AudioTrackId, AudioTrackKind, MonotonicTimestamp, SampleRate,
    };
    use std::sync::Arc;

    #[test]
    fn passthrough_48k() {
        let block = AudioBlock {
            block_id: 0,
            capture_ts: MonotonicTimestamp::ZERO,
            track_id: AudioTrackId::SYSTEM,
            kind: AudioTrackKind::System,
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            samples: Arc::from(vec![0.0f32; 960]),
        };
        let out = resample_to_internal(&block).unwrap();
        assert_eq!(out.samples.len(), 960);
    }

    #[test]
    fn rejects_other_rates() {
        let mut format = AudioFormat::pcm_f32_48k_stereo_interleaved();
        format.sample_rate = SampleRate::new(44_100).unwrap();
        let block = AudioBlock {
            block_id: 0,
            capture_ts: MonotonicTimestamp::ZERO,
            track_id: AudioTrackId::SYSTEM,
            kind: AudioTrackKind::System,
            format,
            samples: Arc::from(vec![0.0f32; 882]),
        };
        assert!(resample_to_internal(&block).is_err());
    }
}
