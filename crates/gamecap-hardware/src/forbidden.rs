//! There is no code path that applies these operations. Callers get an error.

use gamecap_types::{ForbiddenOperation, HardwareError};

/// Always fails. GameCap does not overclock, retune fans, or write firmware.
pub fn deny_hardware_mutation(operation: ForbiddenOperation) -> Result<(), HardwareError> {
    Err(HardwareError::Forbidden {
        operation,
        detail: "GameCap never mutates voltages, clocks, power limits, fans, firmware, BIOS, sysfs or raw devices".to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_forbidden_op_is_rejected() {
        let ops = [
            ForbiddenOperation::SetVoltage,
            ForbiddenOperation::SetClock,
            ForbiddenOperation::SetPowerLimit,
            ForbiddenOperation::SetFanCurve,
            ForbiddenOperation::WriteFirmware,
            ForbiddenOperation::WriteBios,
            ForbiddenOperation::WriteSysfs,
            ForbiddenOperation::RawDeviceIo,
            ForbiddenOperation::AutoOverclock,
        ];
        for op in ops {
            let err = deny_hardware_mutation(op).unwrap_err();
            match err {
                HardwareError::Forbidden { operation, .. } => assert_eq!(operation, op),
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}
