//! Desktop UI (M7 preview): GTK window + in-game HUD in a separate process
//! so the preview never shares threads with capture/encode.

#![forbid(unsafe_code)]

pub const MILESTONE: &str = "M7";
