//! Pure data shaping: 1:1 port of `src/main/services/systemMonitor.ts`
//! (`pickActiveGpu`, `normalizeDiskStats`, `buildSysStats`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const BYTES_PER_GIB: f64 = 1_073_741_824.0; // 1024 ** 3

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskStat {
    pub id: String,
    pub name: String,
    pub filesystem: String,
    pub mount: String,
    /// Usage percent with one decimal (`"50.0"`).
    #[serde(rename = "use")]
    pub use_percent: String,
    /// Size in whole GiB.
    pub size: String,
    /// Used space in whole GiB.
    pub used: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStats {
    /// Utilisation percent.
    pub load: f64,
    /// Temperature in Celsius.
    pub temp: f64,
    /// Total VRAM in MiB (the unit `systeminformation` reports).
    pub mem_total: f64,
    /// Used VRAM in MiB.
    pub mem_used: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessStat {
    pub name: String,
    pub cpu: String,
    pub mem: String,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SysStats {
    pub cpu: String,
    pub ram: String,
    pub ram_text: String,
    pub gpu_name: String,
    pub gpu: Option<GpuStats>,
    pub disks: Vec<DiskStat>,
    pub processes: Vec<ProcessStat>,
}

// ---- inputs (the `*Like` interfaces) ------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CurrentLoad {
    pub current_load: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MemoryUsage {
    pub used: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphicsController {
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub utilization_gpu: Option<f64>,
    pub temperature_gpu: Option<f64>,
    pub memory_total: Option<f64>,
    pub memory_used: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsSize {
    pub fs: Option<String>,
    pub mount: Option<String>,
    pub size: Option<f64>,
    pub used: Option<f64>,
    /// Usage percent (`use` in systeminformation).
    pub use_percent: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSample {
    pub name: String,
    pub cpu: f64,
    pub mem: f64,
    pub pid: u32,
}

// ---- JS number formatting -----------------------------------------------------------------

/// `Number.prototype.toFixed`: rounds half away from zero on the exact binary value
/// (Rust's `{:.N}` rounds ties to even, which differs for e.g. `0.25`).
pub fn to_fixed(value: f64, digits: usize) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    let negative = value < 0.0;
    let exact = format!("{:.*}", digits + 25, value.abs());
    let (int_part, frac_part) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let round_up = frac_part.as_bytes().get(digits).is_some_and(|b| *b >= b'5');
    let mut digits_buf: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes().take(digits))
        .collect();
    if round_up {
        let mut i = digits_buf.len();
        loop {
            if i == 0 {
                digits_buf.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits_buf[i] == b'9' {
                digits_buf[i] = b'0';
            } else {
                digits_buf[i] += 1;
                break;
            }
        }
    }
    let int_len = digits_buf.len() - digits;
    let mut out = String::with_capacity(digits_buf.len() + 2);
    if negative {
        out.push('-');
    }
    out.push_str(&String::from_utf8_lossy(&digits_buf[..int_len]));
    if digits > 0 {
        out.push('.');
        out.push_str(&String::from_utf8_lossy(&digits_buf[int_len..]));
    }
    out
}

fn format_percent(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => to_fixed(v, 1),
        _ => "0.0".to_string(),
    }
}

fn format_gigabytes(value: Option<f64>) -> String {
    to_fixed(value.unwrap_or(0.0) / BYTES_PER_GIB, 0)
}

fn trim_value(value: &Option<String>) -> String {
    value.as_deref().map(str::trim).unwrap_or_default().to_string()
}

fn create_unique_disk_id(base_id: &str, seen: &mut HashSet<String>) -> String {
    let base = if base_id.is_empty() { "disk" } else { base_id };
    let mut next = base.to_string();
    let mut suffix = 2;
    while seen.contains(&next) {
        next = format!("{base}-{suffix}");
        suffix += 1;
    }
    seen.insert(next.clone());
    next
}

/// Prefer an NVIDIA / RTX controller, otherwise the first one.
pub fn pick_active_gpu(controllers: &[GraphicsController]) -> Option<&GraphicsController> {
    controllers
        .iter()
        .find(|c| {
            c.vendor.as_deref().is_some_and(|v| v.to_lowercase().contains("nvidia"))
                || c.model.as_deref().is_some_and(|m| m.to_lowercase().contains("rtx"))
        })
        .or_else(|| controllers.first())
}

/// Keep every disk with a positive size; assign stable, unique ids.
pub fn normalize_disk_stats(disks: &[FsSize]) -> Vec<DiskStat> {
    let mut seen = HashSet::new();
    // Filter first, then number: matches `disks.filter(...).map((disk, index) => ...)` in the TS.
    disks
        .iter()
        .filter(|d| d.size.is_some_and(|s| s.is_finite() && s > 0.0))
        .enumerate()
        .map(|(index, disk)| {
            let filesystem = trim_value(&disk.fs);
            let mount = trim_value(&disk.mount);
            let name = if !mount.is_empty() {
                mount.clone()
            } else if !filesystem.is_empty() {
                filesystem.clone()
            } else {
                format!("Disk {}", index + 1)
            };
            let id_base = [mount.to_lowercase(), filesystem.to_lowercase()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("|");
            let id_base = if id_base.is_empty() { format!("disk-{}", index + 1) } else { id_base };
            DiskStat {
                id: create_unique_disk_id(&id_base, &mut seen),
                name,
                filesystem,
                mount,
                use_percent: format_percent(disk.use_percent),
                size: format_gigabytes(disk.size),
                used: format_gigabytes(disk.used),
            }
        })
        .collect()
}

pub struct BuildInput<'a> {
    pub load: CurrentLoad,
    pub mem: MemoryUsage,
    pub graphics: &'a [GraphicsController],
    pub disks: &'a [FsSize],
    pub processes: &'a [ProcessSample],
}

/// Top five processes by CPU, formatted.
pub(crate) fn top_processes(processes: &[ProcessSample]) -> Vec<ProcessStat> {
    let mut sorted: Vec<&ProcessSample> = processes.iter().collect();
    sorted.sort_by(|a, b| b.cpu.total_cmp(&a.cpu)); // stable, like Array#sort
    sorted
        .into_iter()
        .take(5)
        .map(|p| ProcessStat {
            name: p.name.clone(),
            cpu: to_fixed(p.cpu, 1),
            mem: to_fixed(p.mem, 1),
            pid: p.pid,
        })
        .collect()
}

pub(crate) fn gpu_fields(graphics: &[GraphicsController]) -> (String, Option<GpuStats>) {
    let active = pick_active_gpu(graphics);
    (
        active.and_then(|g| g.model.clone()).unwrap_or_else(|| "N/A".to_string()),
        active.map(|g| GpuStats {
            load: g.utilization_gpu.unwrap_or(0.0),
            temp: g.temperature_gpu.unwrap_or(0.0),
            mem_total: g.memory_total.unwrap_or(0.0),
            mem_used: g.memory_used.unwrap_or(0.0),
        }),
    )
}

pub(crate) fn ram_fields(mem: MemoryUsage) -> (String, String) {
    let percent = if mem.total > 0.0 { mem.used / mem.total * 100.0 } else { 0.0 };
    (
        format_percent(Some(percent)),
        format!("{} / {} GB", to_fixed(mem.used / BYTES_PER_GIB, 1), to_fixed(mem.total / BYTES_PER_GIB, 1)),
    )
}

pub fn build_sys_stats(input: BuildInput<'_>) -> SysStats {
    let (gpu_name, gpu) = gpu_fields(input.graphics);
    let (ram, ram_text) = ram_fields(input.mem);
    SysStats {
        cpu: format_percent(Some(input.load.current_load)),
        ram,
        ram_text,
        gpu_name,
        gpu,
        disks: normalize_disk_stats(input.disks),
        processes: top_processes(input.processes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: f64 = BYTES_PER_GIB;

    fn disk(fs: &str, mount: &str, size_gib: f64, used_gib: f64, use_percent: f64) -> FsSize {
        FsSize {
            fs: Some(fs.into()),
            mount: Some(mount.into()),
            size: Some(size_gib * GIB),
            used: Some(used_gib * GIB),
            use_percent: Some(use_percent),
        }
    }

    // ---- port of src/test/main/systemMonitor.test.ts ----

    #[test]
    fn keeps_every_disk_and_labels_each_entry_separately() {
        let disks = normalize_disk_stats(&[
            disk("C:", "C:", 512.0, 256.0, 50.0),
            disk("D:", "D:", 1024.0, 200.0, 19.5),
        ]);
        assert_eq!(disks.len(), 2);
        let expect = |d: &DiskStat, name: &str, use_percent: &str, size: &str, used: &str| {
            assert_eq!(d.name, name);
            assert_eq!(d.filesystem, name);
            assert_eq!(d.mount, name);
            assert_eq!(d.use_percent, use_percent);
            assert_eq!(d.size, size);
            assert_eq!(d.used, used);
        };
        expect(&disks[0], "C:", "50.0", "512", "256");
        expect(&disks[1], "D:", "19.5", "1024", "200");
        let ids: HashSet<_> = disks.iter().map(|d| d.id.clone()).collect();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn builds_the_monitor_payload_with_all_disks_gpu_and_sorted_processes() {
        let stats = build_sys_stats(BuildInput {
            load: CurrentLoad { current_load: 12.34 },
            mem: MemoryUsage { used: 8.0 * GIB, total: 16.0 * GIB },
            graphics: &[GraphicsController {
                vendor: Some("NVIDIA".into()),
                model: Some("RTX 4070".into()),
                utilization_gpu: Some(44.0),
                temperature_gpu: Some(66.0),
                memory_total: Some(12288.0),
                memory_used: Some(2048.0),
            }],
            disks: &[
                disk("/dev/disk1s1", "/", 500.0, 250.0, 50.0),
                disk("/dev/disk2s1", "/data", 1000.0, 400.0, 40.0),
            ],
            processes: &[
                ProcessSample { name: "helper".into(), cpu: 5.1, mem: 1.5, pid: 3 },
                ProcessSample { name: "renderer".into(), cpu: 18.7, mem: 4.2, pid: 2 },
                ProcessSample { name: "electron".into(), cpu: 30.2, mem: 6.3, pid: 1 },
            ],
        });

        assert_eq!(stats.cpu, "12.3");
        assert_eq!(stats.ram, "50.0");
        assert_eq!(stats.ram_text, "8.0 / 16.0 GB");
        assert_eq!(stats.gpu_name, "RTX 4070");
        assert_eq!(
            stats.gpu,
            Some(GpuStats { load: 44.0, temp: 66.0, mem_total: 12288.0, mem_used: 2048.0 })
        );
        assert_eq!(stats.disks.len(), 2);
        assert_eq!(stats.disks[0].name, "/");
        assert_eq!(stats.disks[0].filesystem, "/dev/disk1s1");
        assert_eq!(stats.disks[0].mount, "/");
        let names: Vec<_> = stats.processes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["electron", "renderer", "helper"]);
        assert_eq!(stats.processes[0].cpu, "30.2");
    }

    // ---- additional coverage of the ported helpers ----

    #[test]
    fn duplicate_disks_get_unique_ids_and_invalid_sizes_are_dropped() {
        let disks = normalize_disk_stats(&[
            disk("/dev/sda1", "/", 100.0, 10.0, 10.0),
            disk("/dev/sda1", "/", 100.0, 10.0, 10.0), // bind mount duplicate
            disk("/dev/sda1", "/", 100.0, 10.0, 10.0),
            FsSize { fs: Some("tmpfs".into()), mount: Some("/run".into()), size: Some(0.0), ..FsSize::default() },
            FsSize { fs: Some("x".into()), mount: Some("/x".into()), size: Some(f64::NAN), ..FsSize::default() },
            FsSize { fs: Some("y".into()), mount: Some("/y".into()), size: None, ..FsSize::default() },
        ]);
        let ids: Vec<_> = disks.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["/|/dev/sda1", "/|/dev/sda1-2", "/|/dev/sda1-3"]);
    }

    #[test]
    fn disk_naming_fallbacks() {
        let disks = normalize_disk_stats(&[
            FsSize { fs: Some(" /dev/x ".into()), mount: Some("  ".into()), size: Some(GIB), ..FsSize::default() },
            FsSize { fs: None, mount: None, size: Some(GIB), ..FsSize::default() },
            FsSize { fs: None, mount: Some("C:\\".into()), size: Some(GIB), ..FsSize::default() },
        ]);
        assert_eq!(disks[0].name, "/dev/x");
        assert_eq!(disks[0].id, "/dev/x");
        assert_eq!(disks[1].name, "Disk 2");
        assert_eq!(disks[1].id, "disk-2");
        assert_eq!(disks[1].use_percent, "0.0");
        assert_eq!(disks[1].used, "0");
        assert_eq!(disks[2].id, "c:\\");
    }

    #[test]
    fn fallback_disk_numbers_count_after_filtering() {
        let disks = normalize_disk_stats(&[
            FsSize { size: Some(0.0), ..FsSize::default() }, // dropped
            FsSize { size: Some(GIB), ..FsSize::default() },
            FsSize { size: Some(GIB), ..FsSize::default() },
        ]);
        let names: Vec<_> = disks.iter().map(|d| (d.name.as_str(), d.id.as_str())).collect();
        assert_eq!(names, [("Disk 1", "disk-1"), ("Disk 2", "disk-2")]);
    }

    #[test]
    fn gpu_selection_and_fallbacks() {
        let intel = GraphicsController { vendor: Some("Intel".into()), model: Some("UHD".into()), ..Default::default() };
        let nv = GraphicsController { vendor: Some("NVIDIA Corporation".into()), model: Some("GTX".into()), ..Default::default() };
        let rtx = GraphicsController { vendor: None, model: Some("Some RTX 3060".into()), ..Default::default() };
        assert_eq!(pick_active_gpu(&[intel.clone(), nv.clone()]), Some(&nv));
        assert_eq!(pick_active_gpu(&[intel.clone(), rtx.clone()]), Some(&rtx));
        assert_eq!(pick_active_gpu(std::slice::from_ref(&intel)), Some(&intel));
        assert_eq!(pick_active_gpu(&[]), None);

        let stats = build_sys_stats(BuildInput {
            load: CurrentLoad::default(),
            mem: MemoryUsage::default(),
            graphics: &[],
            disks: &[],
            processes: &[],
        });
        assert_eq!(stats.gpu_name, "N/A");
        assert_eq!(stats.gpu, None);
        assert_eq!(stats.ram, "0.0");
        assert_eq!(stats.ram_text, "0.0 / 0.0 GB");

        // controller without telemetry: zeros (not null), like the TS defaults
        let stats = build_sys_stats(BuildInput {
            load: CurrentLoad { current_load: f64::NAN },
            mem: MemoryUsage::default(),
            graphics: &[intel],
            disks: &[],
            processes: &[],
        });
        assert_eq!(stats.cpu, "0.0");
        assert_eq!(stats.gpu_name, "UHD");
        assert_eq!(stats.gpu, Some(GpuStats { load: 0.0, temp: 0.0, mem_total: 0.0, mem_used: 0.0 }));
    }

    #[test]
    fn only_top_five_processes_stable_order() {
        let samples: Vec<_> = (0..8)
            .map(|i| ProcessSample { name: format!("p{i}"), cpu: if i < 3 { 1.0 } else { f64::from(i) }, mem: 0.04, pid: i })
            .collect();
        let top = top_processes(&samples);
        let names: Vec<_> = top.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["p7", "p6", "p5", "p4", "p3"]);
        assert_eq!(top[0].mem, "0.0");
    }

    #[test]
    fn to_fixed_matches_javascript() {
        assert_eq!(to_fixed(0.25, 1), "0.3"); // JS: "0.3" (ties away; Rust would say 0.2)
        assert_eq!(to_fixed(0.35, 1), "0.3"); // 0.35 is 0.34999.. in binary
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1.5, 0), "2");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(9.96, 1), "10.0");
        assert_eq!(to_fixed(99.95, 1), "100.0");
        assert_eq!(to_fixed(12.34, 1), "12.3");
        assert_eq!(to_fixed(0.0, 1), "0.0");
        assert_eq!(to_fixed(-0.04, 1), "-0.0");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(512.0, 0), "512");
        assert_eq!(to_fixed(999.5, 0), "1000");
    }
}
