//! Write-path policy. Recording output and sidecars must stay in user space.

use std::path::Path;

use gamecap_types::{ForbiddenOperation, HardwareError};

pub fn is_forbidden_write_path(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    let normalized = raw.replace('\\', "/").to_ascii_lowercase();
    let trimmed = normalized.trim();
    looks_like_raw_device(trimmed)
        || trimmed.starts_with("/sys")
        || trimmed.starts_with("/proc")
        || trimmed.starts_with("/dev")
        || trimmed.starts_with("/boot")
        || trimmed.starts_with("/sys/firmware")
        || trimmed.contains("/firmware/")
        || trimmed.contains("\\\\.\\physicaldrive")
}

fn looks_like_raw_device(path: &str) -> bool {
    path.starts_with("/dev/sd")
        || path.starts_with("/dev/nvme")
        || path.starts_with("/dev/vd")
        || path.starts_with("/dev/loop")
        || path.starts_with("/dev/raw")
        || path == "/dev/mem"
        || path == "/dev/kmem"
}

/// Sidecar/media writes must pass this before `fs::write`.
pub fn assert_user_write_path(path: &Path) -> Result<(), HardwareError> {
    if is_forbidden_write_path(path) {
        return Err(HardwareError::Forbidden {
            operation: ForbiddenOperation::RawDeviceIo,
            detail: format!("refusing to write {}", path.display()),
        });
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && is_forbidden_write_path(parent) {
            return Err(HardwareError::Forbidden {
                operation: ForbiddenOperation::WriteSysfs,
                detail: format!("refusing to write under {}", parent.display()),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_sys_and_raw_devices() {
        assert!(is_forbidden_write_path(Path::new(
            "/sys/class/hwmon/hwmon0/pwm1"
        )));
        assert!(is_forbidden_write_path(Path::new("/dev/nvme0n1")));
        assert!(is_forbidden_write_path(Path::new(
            "/proc/sys/vm/drop_caches"
        )));
        assert!(!is_forbidden_write_path(Path::new(
            "/tmp/gamecap-session.json"
        )));
        assert!(assert_user_write_path(&PathBuf::from("/sys/firmware/efi")).is_err());
    }
}
