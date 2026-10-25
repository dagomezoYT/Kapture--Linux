//! Host-safety contracts. GameCap observes the machine; it never tunes it.

use std::fmt;
use std::path::Path;

/// Operations the recorder must never perform. There is no “debug override”
/// that turns these into real hardware writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ForbiddenOperation {
    SetVoltage,
    SetClock,
    SetPowerLimit,
    SetFanCurve,
    WriteFirmware,
    WriteBios,
    WriteSysfs,
    RawDeviceIo,
    AutoOverclock,
}

impl ForbiddenOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SetVoltage => "set_voltage",
            Self::SetClock => "set_clock",
            Self::SetPowerLimit => "set_power_limit",
            Self::SetFanCurve => "set_fan_curve",
            Self::WriteFirmware => "write_firmware",
            Self::WriteBios => "write_bios",
            Self::WriteSysfs => "write_sysfs",
            Self::RawDeviceIo => "raw_device_io",
            Self::AutoOverclock => "auto_overclock",
        }
    }
}

impl fmt::Display for ForbiddenOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    Forbidden {
        operation: ForbiddenOperation,
        detail: String,
    },
    Unavailable(String),
    Probe(String),
    Privilege(String),
}

impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forbidden { operation, detail } => {
                write!(f, "forbidden hardware operation ({operation}): {detail}")
            }
            Self::Unavailable(msg) => write!(f, "hardware probe unavailable: {msg}"),
            Self::Probe(msg) => write!(f, "hardware probe: {msg}"),
            Self::Privilege(msg) => write!(f, "privilege: {msg}"),
        }
    }
}

impl std::error::Error for HardwareError {}

/// Why the governor stopped or degraded the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SafetyStopReason {
    DiskSpace,
    Memory,
    EncoderSaturation,
    GpuSaturation,
    StorageSaturation,
    Watchdog,
    OutputLimit,
}

impl SafetyStopReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiskSpace => "disk_space",
            Self::Memory => "memory",
            Self::EncoderSaturation => "encoder_saturation",
            Self::GpuSaturation => "gpu_saturation",
            Self::StorageSaturation => "storage_saturation",
            Self::Watchdog => "watchdog",
            Self::OutputLimit => "output_limit",
        }
    }
}

impl fmt::Display for SafetyStopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafetyAction {
    Continue,
    Degrade(SafetyStopReason),
    Stop(SafetyStopReason),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedTemperature {
    pub sensor: String,
    pub celsius: f32,
}

/// Read-only view of host load. Missing fields mean “not consultable”.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostSample {
    pub cpu_load_1m: Option<f32>,
    pub mem_available_bytes: Option<u64>,
    pub mem_total_bytes: Option<u64>,
    pub temperatures_c: Vec<NamedTemperature>,
    pub gpu_busy_percent: Option<f32>,
    pub disk_free_bytes: Option<u64>,
    pub elevated_privileges: bool,
}

/// Read-only free-space probe. Implementations must not write to the volume.
pub trait StorageProbe: Send + Sync {
    fn free_bytes(&self, path: &Path) -> Result<u64, HardwareError>;
}

/// Read-only host telemetry. Must never open sysfs/proc for write.
pub trait HostTelemetry: Send + Sync {
    fn sample(&self) -> HostSample;
}

/// Snapshot used by the governor (pure policy, no OS calls).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceSnapshot {
    pub disk_free_bytes: Option<u64>,
    pub pipeline_bytes: u64,
    pub bytes_written: u64,
    pub video_queue_depth: u64,
    pub video_queue_capacity: u64,
    pub packet_queue_depth: u64,
    pub packet_queue_capacity: u64,
    pub encode_backpressure_drops: u64,
    pub mux_backpressure_drops: u64,
    pub disk_backpressure_drops: u64,
    pub gpu_busy_percent: Option<f32>,
    pub bytes_written_unchanged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_names_are_stable() {
        assert_eq!(ForbiddenOperation::AutoOverclock.as_str(), "auto_overclock");
        assert_eq!(ForbiddenOperation::WriteSysfs.as_str(), "write_sysfs");
    }
}
