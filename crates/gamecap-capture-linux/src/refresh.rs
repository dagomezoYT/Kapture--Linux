//! Detect the active monitor mode: resolution + refresh (GNOME Mutter, then xrandr).

use std::process::Command;

use gamecap_types::FrameRateMode;

/// Native desktop mode used to drive capture and encode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

impl DisplayMode {
    pub fn fallback() -> Self {
        Self {
            width: 1920,
            height: 1080,
            refresh_hz: 60,
        }
    }
}

/// Refresh used for capture pacing. Prefers the session request, then the display.
pub fn refresh_hz_for_request(mode: FrameRateMode) -> u32 {
    if let Some(r) = mode.target_fps() {
        let hz = (f64::from(r.num()) / f64::from(r.den())).round() as u32;
        if (24..=360).contains(&hz) {
            return hz;
        }
    }
    detect_display_mode().refresh_hz
}

/// Active display mode. Falls back to 1920×1080 @ 60 Hz if unknown.
pub fn detect_display_mode() -> DisplayMode {
    if let Some(mode) = mutter_mode() {
        return mode;
    }
    if let Some(mode) = xrandr_mode() {
        return mode;
    }
    DisplayMode::fallback()
}

/// Active display refresh in whole hertz.
pub fn detect_display_refresh_hz() -> u32 {
    detect_display_mode().refresh_hz
}

fn mutter_mode() -> Option<DisplayMode> {
    let gdbus = stdout(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.DisplayConfig",
            "--object-path",
            "/org/gnome/Mutter/DisplayConfig",
            "--method",
            "org.gnome.Mutter.DisplayConfig.GetCurrentState",
        ],
    );
    if let Some(text) = gdbus.as_deref() {
        if let Some(mode) = parse_mutter_current_mode(text) {
            return Some(mode);
        }
    }
    let busctl = stdout(
        "busctl",
        &[
            "--user",
            "call",
            "org.gnome.Mutter.DisplayConfig",
            "/org/gnome/Mutter/DisplayConfig",
            "org.gnome.Mutter.DisplayConfig",
            "GetCurrentState",
        ],
    );
    busctl.as_deref().and_then(parse_mutter_current_mode)
}

fn xrandr_mode() -> Option<DisplayMode> {
    let text = stdout("xrandr", &["--current"])?;
    parse_xrandr_current_mode(&text)
}

fn stdout(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn parse_mutter_current_mode(text: &str) -> Option<DisplayMode> {
    let key = text.find("is-current")?;
    let start = key.saturating_sub(500);
    let window = &text[start..key];
    let at = window.rfind('@')?;
    let hz = parse_hz(&window[at + 1..])?;
    let left = &window[..at];
    let x = left.rfind('x')?;
    let height: u32 = left[x + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    let w_digits: String = left[..x]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let width: u32 = w_digits.parse().ok()?;
    if width < 160 || height < 64 || !(24..=360).contains(&hz) {
        return None;
    }
    Some(DisplayMode {
        width,
        height,
        refresh_hz: hz,
    })
}

fn parse_xrandr_current_mode(text: &str) -> Option<DisplayMode> {
    for line in text.lines() {
        if !line.contains('*') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let res = parts.next()?;
        let (width, height) = parse_res(res)?;
        for tok in parts {
            let trimmed = tok.trim_end_matches(['*', '+']);
            if let Some(hz) = parse_hz(trimmed) {
                return Some(DisplayMode {
                    width,
                    height,
                    refresh_hz: hz,
                });
            }
        }
    }
    None
}

fn parse_res(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn parse_hz(s: &str) -> Option<u32> {
    let num: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let hz: f64 = num.parse().ok()?;
    if (24.0..500.0).contains(&hz) {
        Some(hz.round() as u32)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn mutter_picks_is_current_180() {
        let sample = "('1920x1200@180.001', 1920, 1200, 180.0010986328125, 1.0, [1.0, 2.0], {'is-current': <true>, 'is-preferred': <true>})";
        assert_eq!(
            parse_mutter_current_mode(sample),
            Some(DisplayMode {
                width: 1920,
                height: 1200,
                refresh_hz: 180
            })
        );
    }

    #[test]
    fn xrandr_picks_starred_mode() {
        let sample =
            "eDP-1 connected primary 1920x1200+0+0\n   1920x1200    179.89*+\n   1920x1200     60.00\n";
        assert_eq!(
            parse_xrandr_current_mode(sample),
            Some(DisplayMode {
                width: 1920,
                height: 1200,
                refresh_hz: 180
            })
        );
    }
}
