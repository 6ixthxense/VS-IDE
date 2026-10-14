//! Killing a whole process tree.
//!
//! * Windows: `taskkill /pid <pid> /t /f` (exactly what the TS service does).
//! * Unix: the task is started in its own process group (`process_group(0)`), and the
//!   group receives SIGTERM, then SIGKILL if anything survives for 1.2 s.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::error::TaskError;

/// How long to wait for a process to exit after a termination request.
#[cfg_attr(not(unix), allow(dead_code))]
pub const EXIT_TIMEOUT: Duration = Duration::from_millis(1200);

/// Set by the supervisor thread when the child process was reaped.
#[derive(Default)]
pub struct ExitSignal {
    exited: Mutex<bool>,
    condvar: Condvar,
}

impl ExitSignal {
    pub fn mark_exited(&self) {
        if let Ok(mut exited) = self.exited.lock() {
            *exited = true;
        }
        self.condvar.notify_all();
    }

    pub fn has_exited(&self) -> bool {
        self.exited.lock().map(|e| *e).unwrap_or(true)
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    /// Wait until the process exited; `true` if it did within `timeout`.
    pub fn wait(&self, timeout: Duration) -> bool {
        let Ok(guard) = self.exited.lock() else {
            return true;
        };
        match self
            .condvar
            .wait_timeout_while(guard, timeout, |exited| !*exited)
        {
            Ok((guard, _)) => *guard,
            Err(_) => true,
        }
    }
}

/// Arguments of the Windows helper process: `taskkill /pid <pid> /t /f`.
pub fn taskkill_args(pid: u32) -> Vec<String> {
    vec![
        "/pid".to_string(),
        pid.to_string(),
        "/t".to_string(),
        "/f".to_string(),
    ]
}

/// Interpretation of a `taskkill` run (port of the checks in `terminateChildProcess`).
/// `Ok` when it succeeded, the process was already gone, or the child has exited anyway.
pub fn taskkill_outcome(
    exit_code: Option<i32>,
    output: &str,
    child_exited: bool,
) -> Result<(), TaskError> {
    let lowered = output.to_lowercase();
    let already_gone = lowered.contains("not found")
        || lowered.contains("no running instance")
        || lowered.contains("cannot find the process");

    if exit_code == Some(0) || already_gone || child_exited {
        return Ok(());
    }
    let details = output.trim();
    Err(TaskError::StopFailed(if details.is_empty() {
        format!(
            "taskkill exited with code {}",
            exit_code.map_or_else(|| "unknown".to_string(), |c| c.to_string())
        )
    } else {
        details.to_string()
    }))
}

#[cfg(windows)]
pub fn terminate_tree(pid: u32, exit: &ExitSignal) -> Result<(), TaskError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if exit.has_exited() {
        return Ok(());
    }
    let output = Command::new("taskkill")
        .args(taskkill_args(pid))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| TaskError::StopFailed(error.to_string()))?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    taskkill_outcome(output.status.code(), &text, exit.has_exited())
}

#[cfg(unix)]
fn killpg(pgid: i32, signal: i32) -> std::io::Result<()> {
    // SAFETY: plain syscall wrapper, no memory is touched.
    let result = unsafe { libc::killpg(pgid, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        // Nothing left in the group: that is the goal.
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(unix)]
fn group_exists(pgid: i32) -> bool {
    // SAFETY: signal 0 only checks for existence.
    unsafe { libc::killpg(pgid, 0) == 0 }
}

#[cfg(unix)]
pub fn terminate_tree(pid: u32, exit: &ExitSignal) -> Result<(), TaskError> {
    use std::time::Instant;

    // The child leads its own process group, so pgid == pid.
    let Ok(pgid) = i32::try_from(pid) else {
        return Err(TaskError::StopFailed(format!("invalid pid {pid}")));
    };

    // The leader may already be gone while background members of its group live on:
    // they are still part of the task's process tree.
    let leader_exited = exit.has_exited();
    if leader_exited && !group_exists(pgid) {
        return Ok(());
    }

    killpg(pgid, libc::SIGTERM).map_err(|e| TaskError::StopFailed(e.to_string()))?;

    let leader_gone = leader_exited || exit.wait(EXIT_TIMEOUT);
    // Give the rest of the group the same grace period.
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while group_exists(pgid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    if !leader_gone || group_exists(pgid) {
        killpg(pgid, libc::SIGKILL).map_err(|e| TaskError::StopFailed(e.to_string()))?;
        if !leader_gone && !exit.wait(EXIT_TIMEOUT) {
            return Err(TaskError::StopFailed("process did not exit".into()));
        }
    }
    Ok(())
}

/// SIGKILL the whole process group right away (best effort).
#[cfg(unix)]
pub fn force_kill_group(pid: u32) {
    if let Ok(pgid) = i32::try_from(pid) {
        let _ = killpg(pgid, libc::SIGKILL);
    }
}

#[cfg(windows)]
pub fn force_kill_group(_pid: u32) {
    // `taskkill /T` follows parent links, which no longer exist once the leader exited.
}

#[cfg(not(any(unix, windows)))]
pub fn terminate_tree(_pid: u32, _exit: &ExitSignal) -> Result<(), TaskError> {
    Err(TaskError::StopFailed("unsupported platform".into()))
}

#[cfg(not(any(unix, windows)))]
pub fn force_kill_group(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taskkill_arguments_match_the_ts_service() {
        assert_eq!(taskkill_args(4321), vec!["/pid", "4321", "/t", "/f"]);
    }

    #[test]
    fn taskkill_outcome_accepts_success_gone_processes_and_exited_children() {
        assert!(taskkill_outcome(Some(0), "", false).is_ok());
        assert!(taskkill_outcome(Some(128), "ERROR: The process \"1\" not found.", false).is_ok());
        assert!(taskkill_outcome(Some(1), "No running instance of the task.", false).is_ok());
        assert!(taskkill_outcome(Some(1), "ERROR: Cannot find the process", false).is_ok());
        assert!(taskkill_outcome(Some(1), "denied", true).is_ok());
    }

    #[test]
    fn taskkill_outcome_reports_failures() {
        let error = taskkill_outcome(Some(1), "Access is denied.\n", false).unwrap_err();
        assert!(error.to_string().contains("Access is denied."));
        let error = taskkill_outcome(Some(5), "  ", false).unwrap_err();
        assert!(error.to_string().contains("taskkill exited with code 5"));
        let error = taskkill_outcome(None, "", false).unwrap_err();
        assert!(error.to_string().contains("unknown"));
    }

    #[test]
    fn exit_signal_wakes_waiters() {
        let signal = std::sync::Arc::new(ExitSignal::default());
        assert!(!signal.wait(Duration::from_millis(10)));
        let other = signal.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            other.mark_exited();
        });
        assert!(signal.wait(Duration::from_secs(5)));
        assert!(signal.has_exited());
        handle.join().expect("join");
    }
}
