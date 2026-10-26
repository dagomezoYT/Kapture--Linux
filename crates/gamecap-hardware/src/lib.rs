//! Read-only host observation. Mutation of clocks, voltages, fans, firmware
//! or raw devices is implemented only as a hard error.

#![allow(unsafe_code)] // `statvfs` wrapper only; see `storage.rs`.

pub mod forbidden;
pub mod gpu;
pub mod paths;
pub mod storage;
pub mod telemetry;

pub use forbidden::deny_hardware_mutation;
pub use gpu::{
    allows_ultra, choose_display_gpu, display_allows_ultra, display_render_node, display_vendor,
    probe_gpus, GpuNode, GpuVendor,
};
pub use paths::{assert_user_write_path, is_forbidden_write_path};
pub use storage::{resolve_probe_path, ControllableStorageProbe, OsStorageProbe};
pub use telemetry::OsTelemetry;
