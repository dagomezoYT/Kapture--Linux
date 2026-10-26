"""Pick the GPU that can actually record the screen.

gpu-screen-recorder captures and encodes on the GPU that owns the connected
monitor. On this laptop that is the integrated AMD; the RTX 5070 has no
display wired to it. Forcing the 5070 records black and steals clocks from
the game. A desktop (or an HDMI plugged into the NVIDIA) uses the 5070.
"""

from __future__ import annotations

import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

_CACHE: tuple[float, list["Gpu"]] | None = None
_CACHE_S = 30.0

_VENDOR = {
    "0x10de": "nvidia",
    "0x1002": "amd",
    "0x8086": "intel",
}


@dataclass(frozen=True)
class Gpu:
    card: str
    path: str
    vendor: str
    name: str
    outputs: tuple[str, ...]
    render_node: str = ""
    integrated: bool = False

    @property
    def drives_display(self) -> bool:
        return bool(self.outputs)


def choose_record_gpu(gpus: list[Gpu]) -> Gpu | None:
    """NVIDIA only when it owns a screen. Otherwise the integrated GPU."""
    live = [gpu for gpu in gpus if gpu.drives_display]
    if not live:
        return None
    nvidia = [gpu for gpu in live if gpu.vendor == "nvidia"]
    other = [gpu for gpu in live if gpu.vendor != "nvidia"]
    if nvidia and not other:
        return nvidia[0]
    if other:
        return other[0]
    return nvidia[0]


def describe(gpus: list[Gpu], chosen: Gpu | None) -> str:
    import i18n

    nvidia = next((gpu for gpu in gpus if gpu.vendor == "nvidia"), None)
    if chosen is None:
        return i18n.t("gpu.none")
    if nvidia is not None and chosen.vendor != "nvidia":
        return i18n.t("gpu.hybrid", gpu=chosen.name, spare=nvidia.name)
    if chosen.vendor == "nvidia":
        return i18n.t("gpu.nvidia", gpu=chosen.name)
    return i18n.t("gpu.plain", gpu=chosen.name)


def describe_current() -> str:
    import i18n

    try:
        gpus = discover()
    except OSError:
        return i18n.t("gpu.unreadable")
    return describe(gpus, choose_record_gpu(gpus))


def prepare_for_recording() -> str:
    """Wake NVENC only when the recording GPU is NVIDIA."""
    gpus = discover()
    chosen = choose_record_gpu(gpus)
    if chosen is not None and chosen.vendor == "nvidia":
        _wake_nvidia()
    return describe(gpus, chosen)


def discover() -> list[Gpu]:
    global _CACHE
    now = time.monotonic()
    if _CACHE is not None and now - _CACHE[0] < _CACHE_S:
        return _CACHE[1]
    names = _pci_names()
    found: list[Gpu] = []
    root = Path("/sys/class/drm")
    if not root.is_dir():
        _CACHE = (now, found)
        return found
    for card in sorted(path for path in root.iterdir() if _is_card(path.name)):
        device = card / "device"
        try:
            vendor_id = (device / "vendor").read_text(encoding="utf-8").strip().lower()
        except OSError:
            continue
        vendor = _VENDOR.get(vendor_id, "other")
        pci = device.resolve().name
        raw_name = names.get(pci) or names.get(pci.removeprefix("0000:")) or vendor
        outputs = tuple(sorted(_connected_outputs(root, card.name)))
        found.append(
            Gpu(
                card=card.name,
                path=f"/dev/dri/{card.name}",
                vendor=vendor,
                name=_short_name(vendor, raw_name),
                outputs=outputs,
                render_node=_render_node(device),
                integrated=_integrated(vendor, outputs),
            )
        )
    _CACHE = (now, found)
    return found


def _is_card(name: str) -> bool:
    return name.startswith("card") and name[4:].isdigit()


_PANEL = ("eDP", "LVDS", "DSI", "DPI")


def _integrated(vendor: str, outputs: tuple[str, ...]) -> bool:
    panel = any(name.startswith(_PANEL) for name in outputs)
    if vendor == "nvidia":
        return False
    if vendor in {"amd", "intel"}:
        return panel
    return True


def _render_node(device: Path) -> str:
    drm = device / "drm"
    try:
        names = sorted(path.name for path in drm.iterdir() if path.name.startswith("renderD"))
    except OSError:
        return ""
    if not names:
        return ""
    return f"/dev/dri/{names[0]}"


def _connected_outputs(root: Path, card: str) -> list[str]:
    prefix = f"{card}-"
    outputs: list[str] = []
    for conn in root.iterdir():
        if not conn.name.startswith(prefix) or "Writeback" in conn.name:
            continue
        try:
            status = (conn / "status").read_text(encoding="utf-8").strip()
        except OSError:
            continue
        if status == "connected":
            outputs.append(conn.name[len(prefix) :])
    return outputs


def _pci_names() -> dict[str, str]:
    try:
        out = subprocess.run(
            ["lspci", "-mm"],
            capture_output=True,
            text=True,
            timeout=2,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {}
    names: dict[str, str] = {}
    for line in (out.stdout or "").splitlines():
        parts = _quoted(line)
        if len(parts) < 4:
            continue
        slot, _klass, _vendor, device = parts[0], parts[1], parts[2], parts[3]
        names[slot] = device
        names[f"0000:{slot}"] = device
    return names


def _quoted(line: str) -> list[str]:
    parts: list[str] = []
    buf: list[str] = []
    inside = False
    for char in line:
        if char == '"':
            if inside:
                parts.append("".join(buf))
                buf = []
            inside = not inside
            continue
        if inside:
            buf.append(char)
    if line and not line.startswith('"'):
        slot = line.split(" ", 1)[0]
        parts.insert(0, slot)
    return parts


def _short_name(vendor: str, device: str) -> str:
    text = device.strip()
    if "5070" in text:
        return "RTX 5070"
    if vendor == "nvidia":
        if "[" in text and "]" in text:
            return text[text.find("[") + 1 : text.find("]")]
        return text or "NVIDIA"
    if vendor == "amd":
        return text if text.lower().startswith("amd") else f"AMD {text}"
    if vendor == "intel":
        return text if text.lower().startswith("intel") else f"Intel {text}"
    return text or vendor


def _wake_nvidia() -> None:
    try:
        subprocess.run(
            ["nvidia-smi", "--query-gpu=utilization.gpu,pstate", "--format=csv,noheader"],
            check=False,
            capture_output=True,
            timeout=2,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass
