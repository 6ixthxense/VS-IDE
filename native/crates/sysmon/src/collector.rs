//! Synchronous sampler owning one `sysinfo::System`.

use std::time::{Duration, Instant};

use sysinfo::{
    DiskRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System,
    MINIMUM_CPU_UPDATE_INTERVAL,
};

use crate::gpu::GpuProbe;
use crate::stats::{
    gpu_fields, normalize_disk_stats, ram_fields, top_processes, CurrentLoad, DiskStat, FsSize,
    MemoryUsage, ProcessSample, ProcessStat, SysStats,
};

/// Disks and processes are refreshed at most this often.
pub const DEFAULT_SLOW_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// Cadence of the expensive refreshes (disks + process table).
    pub slow_interval: Duration,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self { slow_interval: DEFAULT_SLOW_INTERVAL }
    }
}

/// Keeps the previous disk / process results between slow refreshes.
///
/// CPU usage in `sysinfo` is a delta between two refreshes at least
/// [`Collector::MIN_CPU_INTERVAL`] apart, so call [`Collector::prime`] (or just construct the
/// collector), wait that long, and only then [`Collector::collect`].
pub struct Collector {
    sys: System,
    disks: Disks,
    gpu: GpuProbe,
    config: CollectorConfig,
    logical_cpus: f64,
    last_slow: Option<Instant>,
    cached_disks: Vec<DiskStat>,
    cached_processes: Vec<ProcessStat>,
}

fn process_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing().with_cpu().with_memory()
}

impl Collector {
    /// Minimum spacing between two CPU refreshes for meaningful numbers.
    pub const MIN_CPU_INTERVAL: Duration = MINIMUM_CPU_UPDATE_INTERVAL;

    pub fn new(config: CollectorConfig) -> Self {
        let mut collector = Self {
            sys: System::new(),
            disks: Disks::new(),
            gpu: GpuProbe::start(),
            config,
            logical_cpus: 1.0,
            last_slow: None,
            cached_disks: Vec::new(),
            cached_processes: Vec::new(),
        };
        collector.prime();
        collector
    }

    /// Take the baseline samples that the next [`Self::collect`] diffs against.  Also forces
    /// the next `collect` to refresh disks and processes.
    pub fn prime(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, process_kind());
        self.logical_cpus = self.sys.cpus().len().max(1) as f64;
        self.last_slow = None;
    }

    /// Sample now.  CPU + RAM (+ GPU when available) are always refreshed; disks and
    /// processes only when the slow interval elapsed.
    pub fn collect(&mut self) -> SysStats {
        self.sys.refresh_cpu_usage();
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());

        let slow_due = self
            .last_slow
            .is_none_or(|t| t.elapsed() >= self.config.slow_interval);
        if slow_due {
            self.refresh_slow();
            self.last_slow = Some(Instant::now());
        }

        let total = self.sys.total_memory() as f64;
        let used = self.sys.used_memory() as f64;
        let controllers = self.gpu.controllers();
        let (gpu_name, gpu) = gpu_fields(&controllers);
        let (ram, ram_text) = ram_fields(MemoryUsage { used, total });

        SysStats {
            cpu: crate::stats::to_fixed(f64::from(self.sys.global_cpu_usage()), 1),
            ram,
            ram_text,
            gpu_name,
            gpu,
            disks: self.cached_disks.clone(),
            processes: self.cached_processes.clone(),
        }
    }

    /// CPU load as the pure-helper input type (useful for tests / custom shaping).
    pub fn current_load(&self) -> CurrentLoad {
        CurrentLoad { current_load: f64::from(self.sys.global_cpu_usage()) }
    }

    fn refresh_slow(&mut self) {
        self.disks
            .refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
        let fs: Vec<FsSize> = self
            .disks
            .iter()
            .map(|d| {
                let size = d.total_space() as f64;
                let used = size - d.available_space() as f64;
                FsSize {
                    fs: Some(d.name().to_string_lossy().into_owned()),
                    mount: Some(d.mount_point().to_string_lossy().into_owned()),
                    size: Some(size),
                    used: Some(used),
                    use_percent: Some(if size > 0.0 { used / size * 100.0 } else { 0.0 }),
                }
            })
            .collect();
        self.cached_disks = normalize_disk_stats(&fs);

        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, process_kind());
        let total_mem = self.sys.total_memory() as f64;
        let samples: Vec<ProcessSample> = self
            .sys
            .processes()
            .values()
            .map(|p| ProcessSample {
                name: p.name().to_string_lossy().into_owned(),
                // sysinfo reports per-core percent (can exceed 100); normalise to total CPU.
                cpu: f64::from(p.cpu_usage()) / self.logical_cpus,
                mem: if total_mem > 0.0 { p.memory() as f64 / total_mem * 100.0 } else { 0.0 },
                pid: p.pid().as_u32(),
            })
            .collect();
        self.cached_processes = top_processes(&samples);
    }
}

