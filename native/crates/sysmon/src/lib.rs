//! vsm-sysmon — replaces `systeminformation` (`systemMonitor.ts`) and the polling loop in
//! `main.ts`.
//!
//! * [`stats`]: the output shape ([`SysStats`]) plus the pure helpers
//!   [`build_sys_stats`], [`normalize_disk_stats`], [`pick_active_gpu`] (1:1 ports).
//! * [`Collector`]: owns one `sysinfo::System`; refreshes CPU + RAM on every call and disks /
//!   processes only on a slower cadence.
//! * [`SysMonitor`]: background thread that drives a [`Collector`] and hands [`SysStats`] to a
//!   callback; `pause()` parks the thread on a channel (zero work), `resume()` wakes it.
//! * Optional cargo feature `nvidia`: GPU load / temperature / VRAM through NVML.

mod collector;
mod gpu;
mod monitor;
pub mod stats;

pub use collector::{Collector, CollectorConfig, DEFAULT_SLOW_INTERVAL};
pub use monitor::{MonitorConfig, SysMonitor, DEFAULT_INTERVAL};
pub use stats::{
    build_sys_stats, normalize_disk_stats, pick_active_gpu, BuildInput, CurrentLoad, DiskStat,
    FsSize, GpuStats, GraphicsController, MemoryUsage, ProcessSample, ProcessStat, SysStats,
};

/// Errors of this crate.  Collection itself is infallible (missing data degrades to defaults).
#[derive(Debug, thiserror::Error)]
pub enum SysMonError {
    #[error("failed to spawn the system monitor thread: {0}")]
    Spawn(#[from] std::io::Error),
}
