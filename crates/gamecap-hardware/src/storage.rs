//! Free-space probes. Read-only. No TRIM, no raw block I/O, no mount changes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gamecap_types::{HardwareError, StorageProbe};

/// Live probe used in production. Unix: `statvfs`. Other OS: unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsStorageProbe;

impl StorageProbe for OsStorageProbe {
    fn free_bytes(&self, path: &Path) -> Result<u64, HardwareError> {
        free_bytes_os(path)
    }
}

/// Test double whose remaining bytes can shrink while a session runs.
#[derive(Debug, Clone)]
pub struct ControllableStorageProbe {
    remaining: Arc<AtomicU64>,
}

impl ControllableStorageProbe {
    pub fn new(bytes: u64) -> Self {
        Self {
            remaining: Arc::new(AtomicU64::new(bytes)),
        }
    }

    pub fn set_remaining(&self, bytes: u64) {
        self.remaining.store(bytes, Ordering::Release);
    }
}

impl StorageProbe for ControllableStorageProbe {
    fn free_bytes(&self, _path: &Path) -> Result<u64, HardwareError> {
        Ok(self.remaining.load(Ordering::Acquire))
    }
}

pub fn resolve_probe_path(path: Option<&Path>) -> PathBuf {
    match path {
        Some(p) if p.as_os_str().is_empty() => std::env::temp_dir(),
        Some(p) => {
            if p.exists() {
                p.to_path_buf()
            } else {
                match p.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                    _ => std::env::temp_dir(),
                }
            }
        }
        None => std::env::temp_dir(),
    }
}

#[cfg(unix)]
fn free_bytes_os(path: &Path) -> Result<u64, HardwareError> {
    use std::ffi::CString;

    let probe = resolve_probe_path(Some(path));
    let c_path = CString::new(probe.to_string_lossy().as_bytes()).map_err(|_| {
        HardwareError::Probe(format!("path contains interior NUL: {}", probe.display()))
    })?;
    // SAFETY: `c_path` is a valid C string that lives for the call. `stat` is
    // stack-allocated and only read after a 0 return. This is a query, not a write.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return Err(HardwareError::Probe(format!(
            "statvfs failed for {}",
            probe.display()
        )));
    }
    let frsize = stat.f_frsize as u64;
    let bavail = stat.f_bavail as u64;
    Ok(bavail.saturating_mul(frsize))
}

#[cfg(not(unix))]
fn free_bytes_os(_path: &Path) -> Result<u64, HardwareError> {
    Err(HardwareError::Unavailable(
        "disk free-space probe is not implemented on this OS yet".to_string(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn controllable_probe_can_shrink() {
        let probe = ControllableStorageProbe::new(1000);
        assert_eq!(probe.free_bytes(Path::new(".")).unwrap(), 1000);
        probe.set_remaining(10);
        assert_eq!(probe.free_bytes(Path::new(".")).unwrap(), 10);
    }

    #[cfg(unix)]
    #[test]
    fn os_probe_returns_nonzero_for_temp() {
        let probe = OsStorageProbe;
        let free = probe.free_bytes(&std::env::temp_dir()).unwrap();
        assert!(free > 0);
    }
}
