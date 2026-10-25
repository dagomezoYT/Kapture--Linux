//! Backpressure and quality policies. Drops are classified, never hidden.

use gamecap_types::DropReason;

use crate::queue::TrySendError;

/// Outcome of offering one item to a bounded queue from the capture thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferOutcome {
    Queued,
    Dropped(DropReason),
    Closed,
}

/// Capture must not block on disk. A full video/encode queue becomes
/// `encode_backpressure` after optional short yields.
#[allow(clippy::result_large_err)]
pub fn offer_from_capture<T: Send>(
    send: impl Fn(T) -> Result<(), TrySendError<T>>,
    mut item: T,
    retries: u32,
    reason_if_full: DropReason,
) -> OfferOutcome {
    let mut attempt = 0u32;
    loop {
        match send(item) {
            Ok(()) => return OfferOutcome::Queued,
            Err(TrySendError::Closed(_)) => return OfferOutcome::Closed,
            Err(TrySendError::Full(returned)) => {
                item = returned;
                if attempt >= retries {
                    return OfferOutcome::Dropped(reason_if_full);
                }
                attempt = attempt.saturating_add(1);
                std::thread::yield_now();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::queue::bounded;

    #[test]
    fn full_queue_is_encode_backpressure() {
        let (tx, _rx) = bounded::<u8>(1);
        tx.try_send(1).unwrap();
        let outcome = offer_from_capture(|v| tx.try_send(v), 2, 0, DropReason::EncodeBackpressure);
        assert_eq!(
            outcome,
            OfferOutcome::Dropped(DropReason::EncodeBackpressure)
        );
    }

    #[test]
    fn queued_when_capacity_remains() {
        let (tx, _rx) = bounded::<u8>(2);
        let outcome = offer_from_capture(|v| tx.try_send(v), 1, 0, DropReason::EncodeBackpressure);
        assert_eq!(outcome, OfferOutcome::Queued);
    }
}
