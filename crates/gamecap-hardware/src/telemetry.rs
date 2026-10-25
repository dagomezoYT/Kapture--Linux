//! Optional host load/temperature sampling. Opens files read-only only.

use std::fs;
use std::path::Path;

use gamecap_types::{HostSample, HostTelemetry, NamedTemperature};

#[derive(Debug, Default, Clone, Copy)]
pub struct OsTelemetry;

impl HostTelemetry for OsTelemetry {
    fn sample(&self) -> HostSample {
        HostSample {
            cpu_load_1m: read_loadavg_1m(),
            mem_available_bytes: read_meminfo_kb("MemAvailable").map(|kb| kb.saturating_mul(1024)),
            mem_total_bytes: read_meminfo_kb("MemTotal").map(|kb| kb.saturating_mul(1024)),
            temperatures_c: read_hwmon_temps(),
            gpu_busy_percent: read_gpu_busy_percent(),
            disk_free_bytes: None,
            elevated_privileges: running_elevated(),
        }
    }
}

fn running_elevated() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and does not mutate the system.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn read_loadavg_1m() -> Option<f32> {
    let text = fs::read_to_string("/proc/loadavg").ok()?;
    let first = text.split_whitespace().next()?;
    first.parse().ok()
}

fn read_meminfo_kb(key: &str) -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim().trim_start_matches(':').trim();
            let num = rest.split_whitespace().next()?;
            return num.parse().ok();
        }
    }
    None
}

fn read_hwmon_temps() -> Vec<NamedTemperature> {
    let mut out = Vec::new();
    let hwmon = Path::new("/sys/class/hwmon");
    let entries = match fs::read_dir(hwmon) {
        Ok(dir) => dir,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let chip = fs::read_to_string(dir.join("name"))
            .unwrap_or_else(|_| "hwmon".to_string())
            .trim()
            .to_string();
        let files = match fs::read_dir(&dir) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for file in files.flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("temp") || !name.ends_with("_input") {
                continue;
            }
            let millideg: i64 = match fs::read_to_string(file.path()) {
                Ok(s) => s.trim().parse().unwrap_or(0),
                Err(_) => continue,
            };
            let label_path = dir.join(name.replace("_input", "_label"));
            let label = fs::read_to_string(label_path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| name.to_string());
            out.push(NamedTemperature {
                sensor: format!("{chip}/{label}"),
                celsius: millideg as f32 / 1000.0,
            });
        }
    }
    out
}

fn read_gpu_busy_percent() -> Option<f32> {
    let drm = Path::new("/sys/class/drm");
    let entries = match fs::read_dir(drm) {
        Ok(dir) => dir,
        Err(_) => return None,
    };
    let mut fallback = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let Ok(text) = fs::read_to_string(device.join("gpu_busy_percent")) else {
            continue;
        };
        let Ok(value) = text.trim().parse::<f32>() else {
            continue;
        };
        let boot_vga = fs::read_to_string(device.join("boot_vga"))
            .ok()
            .is_some_and(|s| s.trim() == "1");
        if boot_vga {
            return Some(value);
        }
        if fallback.is_none() {
            fallback = Some(value);
        }
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_does_not_panic() {
        let sample = OsTelemetry.sample();
        let _ = sample.cpu_load_1m;
        let _ = sample.temperatures_c.len();
    }
}
