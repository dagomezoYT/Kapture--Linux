//! Display GPUs observed from sysfs. Read-only: this module never writes a node.

use std::fs;
use std::path::Path;

/// PCI vendor of a DRM card. Unknown ids stay `Other` so a new GPU still records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    pub fn from_pci(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "0x10de" => Self::Nvidia,
            "0x1002" => Self::Amd,
            "0x8086" => Self::Intel,
            _ => Self::Other,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nvidia => "nvidia",
            Self::Amd => "amd",
            Self::Intel => "intel",
            Self::Other => "other",
        }
    }
}

/// One DRM card. `outputs` lists connected connectors (`eDP-1`, `HDMI-A-1`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuNode {
    pub card: String,
    pub vendor: GpuVendor,
    pub outputs: Vec<String>,
    pub render_node: Option<String>,
}

/// GPU that should encode the screen.
///
/// NVIDIA is used only when it is the only card with a connected output.
/// A hybrid laptop (discrete NVIDIA with no display, integrated GPU on the
/// panel) therefore keeps recording on the panel GPU and leaves the discrete
/// card for the game. That is the same rule as the desktop probe.
pub fn choose_display_gpu(gpus: &[GpuNode]) -> Option<&GpuNode> {
    let mut nvidia: Option<&GpuNode> = None;
    let mut other: Option<&GpuNode> = None;
    for gpu in gpus {
        if gpu.outputs.is_empty() {
            continue;
        }
        if gpu.vendor == GpuVendor::Nvidia {
            if nvidia.is_none() {
                nvidia = Some(gpu);
            }
        } else if other.is_none() {
            other = Some(gpu);
        }
    }
    match (nvidia, other) {
        (Some(nv), None) => Some(nv),
        (_, Some(panel)) => Some(panel),
        (None, None) => None,
    }
}

/// True when the display GPU can hold the Ultra preset.
///
/// Integrated AMD/Intel (panel connector) cannot: that is the baseline this
/// recorder was tuned on. Discrete AMD/Intel and any NVIDIA that owns the
/// screen can.
pub fn display_allows_ultra() -> bool {
    let gpus = probe_gpus();
    allows_ultra(choose_display_gpu(&gpus))
}

pub fn allows_ultra(gpu: Option<&GpuNode>) -> bool {
    let Some(gpu) = gpu else {
        return false;
    };
    match gpu.vendor {
        GpuVendor::Nvidia => true,
        GpuVendor::Amd | GpuVendor::Intel => !has_panel(&gpu.outputs),
        GpuVendor::Other => false,
    }
}

fn has_panel(outputs: &[String]) -> bool {
    outputs.iter().any(|name| {
        name.starts_with("eDP")
            || name.starts_with("LVDS")
            || name.starts_with("DSI")
            || name.starts_with("DPI")
    })
}

/// Vendor of the display GPU, or `Other` when no connector is connected.
pub fn display_vendor() -> GpuVendor {
    let gpus = probe_gpus();
    choose_display_gpu(&gpus)
        .map(|gpu| gpu.vendor)
        .unwrap_or(GpuVendor::Other)
}

/// Render node of the display GPU (`/dev/dri/renderD128`), if sysfs lists one.
pub fn display_render_node() -> Option<String> {
    let gpus = probe_gpus();
    choose_display_gpu(&gpus).and_then(|gpu| gpu.render_node.clone())
}

pub fn probe_gpus() -> Vec<GpuNode> {
    #[cfg(target_os = "linux")]
    {
        probe_gpus_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn probe_gpus_linux() -> Vec<GpuNode> {
    let root = Path::new("/sys/class/drm");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut cards: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_card(&name) {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    cards.sort();
    cards
        .into_iter()
        .filter_map(|card| read_card(root, &card))
        .collect()
}

fn is_card(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("card") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(target_os = "linux")]
fn read_card(root: &Path, card: &str) -> Option<GpuNode> {
    let device = root.join(card).join("device");
    let vendor_txt = fs::read_to_string(device.join("vendor")).ok()?;
    Some(GpuNode {
        card: card.to_string(),
        vendor: GpuVendor::from_pci(&vendor_txt),
        outputs: connected_outputs(root, card),
        render_node: render_node(&device),
    })
}

#[cfg(target_os = "linux")]
fn connected_outputs(root: &Path, card: &str) -> Vec<String> {
    let prefix = format!("{card}-");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut outputs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || name.contains("Writeback") {
            continue;
        }
        let status = fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        if status.trim() == "connected" {
            outputs.push(name[prefix.len()..].to_string());
        }
    }
    outputs.sort();
    outputs
}

#[cfg(target_os = "linux")]
fn render_node(device: &Path) -> Option<String> {
    let entries = fs::read_dir(device.join("drm")).ok()?;
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("renderD") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .next()
        .map(|name| format!("/dev/dri/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(card: &str, vendor: GpuVendor, outputs: &[&str]) -> GpuNode {
        GpuNode {
            card: card.to_string(),
            vendor,
            outputs: outputs.iter().map(|name| (*name).to_string()).collect(),
            render_node: None,
        }
    }

    #[test]
    fn hybrid_laptop_records_on_the_panel_gpu() {
        let gpus = vec![
            node("card0", GpuVendor::Nvidia, &[]),
            node("card1", GpuVendor::Amd, &["eDP-1"]),
        ];
        let chosen = choose_display_gpu(&gpus);
        assert_eq!(chosen.map(|gpu| gpu.card.as_str()), Some("card1"));
        assert_eq!(chosen.map(|gpu| gpu.vendor), Some(GpuVendor::Amd));
    }

    #[test]
    fn nvidia_desktop_records_on_nvidia() {
        let gpus = vec![node("card0", GpuVendor::Nvidia, &["HDMI-A-1"])];
        assert_eq!(
            choose_display_gpu(&gpus).map(|gpu| gpu.vendor),
            Some(GpuVendor::Nvidia)
        );
    }

    #[test]
    fn panel_wins_when_both_gpus_have_outputs() {
        let gpus = vec![
            node("card0", GpuVendor::Nvidia, &["HDMI-A-1"]),
            node("card1", GpuVendor::Intel, &["eDP-1"]),
        ];
        assert_eq!(
            choose_display_gpu(&gpus).map(|gpu| gpu.vendor),
            Some(GpuVendor::Intel)
        );
    }

    #[test]
    fn ultra_follows_a_stronger_display_gpu() {
        let laptop = vec![
            node("card0", GpuVendor::Nvidia, &[]),
            node("card1", GpuVendor::Amd, &["eDP-1"]),
        ];
        assert!(!allows_ultra(choose_display_gpu(&laptop)));
        let desktop_amd = vec![node("card0", GpuVendor::Amd, &["HDMI-A-1"])];
        assert!(allows_ultra(choose_display_gpu(&desktop_amd)));
        let desktop_nv = vec![node("card0", GpuVendor::Nvidia, &["DP-1"])];
        assert!(allows_ultra(choose_display_gpu(&desktop_nv)));
        let intel_panel = vec![node("card0", GpuVendor::Intel, &["eDP-1"])];
        assert!(!allows_ultra(choose_display_gpu(&intel_panel)));
    }

    #[test]
    fn pci_ids_cover_the_three_desktop_vendors() {
        assert_eq!(GpuVendor::from_pci("0x10de"), GpuVendor::Nvidia);
        assert_eq!(GpuVendor::from_pci("0x1002\n"), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_pci("0x8086"), GpuVendor::Intel);
        assert_eq!(GpuVendor::from_pci("0x1af4"), GpuVendor::Other);
    }
}
