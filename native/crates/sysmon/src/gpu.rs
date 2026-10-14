//! GPU discovery.  The name is detected once (off-thread, best effort); live load /
//! temperature / VRAM come from NVML when the `nvidia` feature is enabled and a driver is present.

use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

use crate::stats::GraphicsController;

pub(crate) struct GpuProbe {
    /// Controllers discovered by the OS-specific, one-shot detection thread.
    detected: Arc<OnceLock<Vec<GraphicsController>>>,
    #[cfg(feature = "nvidia")]
    nvml: Option<nvml_wrapper::Nvml>,
}

fn run_text(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command.output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn controller_from_name(name: &str) -> GraphicsController {
    let lowered = name.to_lowercase();
    let vendor = if lowered.contains("nvidia") || lowered.contains("geforce") || lowered.contains("rtx") {
        Some("NVIDIA".to_string())
    } else if lowered.contains("amd") || lowered.contains("radeon") {
        Some("AMD".to_string())
    } else if lowered.contains("intel") {
        Some("Intel".to_string())
    } else {
        None
    };
    GraphicsController { vendor, model: Some(name.trim().to_string()), ..GraphicsController::default() }
}

/// Split `"a" "b" "c"` (lspci -mm) into fields.
fn quoted_fields(line: &str) -> Vec<&str> {
    line.split('"').skip(1).step_by(2).collect()
}

fn detect_controllers() -> Vec<GraphicsController> {
    let names: Vec<String> = if cfg!(windows) {
        run_text(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command",
              "Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }"],
        )
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        run_text("system_profiler", &["SPDisplaysDataType"])
            .map(|t| {
                t.lines()
                    .filter_map(|l| l.trim().strip_prefix("Chipset Model:"))
                    .map(|n| n.trim().to_string())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        run_text("lspci", &["-mm"])
            .map(|t| {
                t.lines()
                    .filter_map(|line| {
                        let fields = quoted_fields(line);
                        let class = fields.first()?.to_lowercase();
                        if !(class.contains("vga") || class.contains("3d") || class.contains("display")) {
                            return None;
                        }
                        Some(format!("{} {}", fields.get(1)?, fields.get(2)?))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    names.iter().map(|n| controller_from_name(n)).collect()
}

impl GpuProbe {
    pub(crate) fn start() -> Self {
        #[cfg(feature = "nvidia")]
        let nvml = nvml_wrapper::Nvml::init().ok().filter(|n| n.device_count().unwrap_or(0) > 0);

        let detected = Arc::new(OnceLock::new());
        #[cfg(feature = "nvidia")]
        let need_detection = nvml.is_none();
        #[cfg(not(feature = "nvidia"))]
        let need_detection = true;

        if need_detection {
            let slot = Arc::clone(&detected);
            // Detached one-shot thread: spawning `powershell`/`lspci` must not delay the first sample.
            let spawned = std::thread::Builder::new()
                .name("vsm-sysmon-gpu-name".into())
                .spawn(move || {
                    let _ = slot.set(detect_controllers());
                });
            if spawned.is_err() {
                let _ = detected.set(Vec::new());
            }
        } else {
            let _ = detected.set(Vec::new());
        }

        Self {
            detected,
            #[cfg(feature = "nvidia")]
            nvml,
        }
    }

    /// Current controller list (empty until detection finished or when no GPU is found).
    pub(crate) fn controllers(&self) -> Vec<GraphicsController> {
        #[cfg(feature = "nvidia")]
        if let Some(controller) = self.nvml_controller() {
            return vec![controller];
        }
        self.detected.get().cloned().unwrap_or_default()
    }

    #[cfg(feature = "nvidia")]
    fn nvml_controller(&self) -> Option<GraphicsController> {
        use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
        const MIB: f64 = 1024.0 * 1024.0;
        let device = self.nvml.as_ref()?.device_by_index(0).ok()?;
        let memory = device.memory_info().ok();
        Some(GraphicsController {
            vendor: Some("NVIDIA".into()),
            model: device.name().ok(),
            utilization_gpu: device.utilization_rates().ok().map(|u| f64::from(u.gpu)),
            temperature_gpu: device.temperature(TemperatureSensor::Gpu).ok().map(f64::from),
            memory_total: memory.as_ref().map(|m| m.total as f64 / MIB),
            memory_used: memory.as_ref().map(|m| m.used as f64 / MIB),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lspci_machine_output_and_vendors() {
        let line = r#"01:00.0 "VGA compatible controller" "NVIDIA Corporation" "AD104 [GeForce RTX 4070]" -ra1 "ASUS" "x""#;
        let fields = quoted_fields(line);
        assert_eq!(&fields[..3], ["VGA compatible controller", "NVIDIA Corporation", "AD104 [GeForce RTX 4070]"]);
        assert_eq!(controller_from_name("NVIDIA GeForce RTX 4070").vendor.as_deref(), Some("NVIDIA"));
        assert_eq!(controller_from_name("Intel UHD Graphics").vendor.as_deref(), Some("Intel"));
        assert_eq!(controller_from_name("Mystery").vendor, None);
    }
}
