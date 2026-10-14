//! Task execution (port of `TaskService` in `main/services/tasks.ts`).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::discovery::discover_package_script_tasks;
use crate::error::{Result, TaskError};
use crate::kill::{force_kill_group, terminate_tree, ExitSignal};
use crate::timeutil::{now_iso, now_millis};
use crate::types::{
    OutputStream, TaskDefinition, TaskRun, TaskRunRequest, TaskRunStatus, TaskUpdate,
};

/// How long to keep draining stdout/stderr after the main process exited (a background
/// grandchild may keep the pipes open forever).
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);

/// Capacity of the update channel created by [`TaskService::channel`].
pub const UPDATE_CHANNEL_CAPACITY: usize = 1024;

/// Longest line (in bytes) delivered as one `Output` update; longer lines are split.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

struct ActiveRun {
    pid: u32,
    stopped_by_user: AtomicBool,
    exit: ExitSignal,
}

#[derive(Default)]
struct Registry {
    runs: HashMap<String, Arc<ActiveRun>>,
    run_by_terminal: HashMap<String, String>,
}

struct Inner {
    registry: Mutex<Registry>,
    updates: SyncSender<TaskUpdate>,
}

/// Runs tasks and reports [`TaskUpdate`]s through a channel. Cheap to clone.
#[derive(Clone)]
pub struct TaskService {
    inner: Arc<Inner>,
}

fn random_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let mut x = (now_millis() as u64)
        ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ u64::from(std::process::id()).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    (0..6)
        .map(|i| DIGITS[((x >> (i * 8)) % 36) as usize] as char)
        .collect()
}

fn build_command(task: &TaskDefinition, cwd: &Path) -> Command {
    let use_shell = task.shell.unwrap_or(cfg!(windows));
    let mut command;

    if use_shell {
        let line = task.command_line();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let comspec = std::env::var("ComSpec")
                .or_else(|_| std::env::var("COMSPEC"))
                .unwrap_or_else(|_| "cmd.exe".to_string());
            command = Command::new(comspec);
            command.raw_arg(format!("/d /s /c \"{line}\""));
        }
        #[cfg(not(windows))]
        {
            command = Command::new("/bin/sh");
            command.arg("-c").arg(line);
        }
    } else {
        command = Command::new(&task.command);
        command.args(&task.args);
    }

    command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group so that stop can signal the whole tree.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Splits a byte stream into output lines. A line ends at `\n`, `\r\n` or a bare `\r`
/// (progress bars rewrite the current line with `\r`, so each rewrite is a line), and is
/// flushed early once it reaches [`MAX_LINE_BYTES`]. Memory use is bounded by that limit.
#[derive(Default)]
struct LineSplitter {
    pending: Vec<u8>,
    /// A `\r` was seen and not yet resolved: `\r\n` is one terminator, anything else
    /// makes it a bare carriage return.
    pending_cr: bool,
}

impl LineSplitter {
    fn emit_all(&mut self, emit: &mut impl FnMut(String)) {
        emit(String::from_utf8_lossy(&self.pending).into_owned());
        self.pending.clear();
    }

    fn resolve_bare_cr(&mut self, emit: &mut impl FnMut(String)) {
        if self.pending_cr {
            self.pending_cr = false;
            if !self.pending.is_empty() {
                self.emit_all(emit);
            }
        }
    }

    /// Flush a full buffer without cutting a UTF-8 sequence in half.
    fn emit_chunk(&mut self, emit: &mut impl FnMut(String)) {
        let cut = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to().max(1),
            Err(_) => self.pending.len(),
        };
        let rest = self.pending.split_off(cut);
        emit(String::from_utf8_lossy(&self.pending).into_owned());
        self.pending = rest;
    }

    fn push(&mut self, chunk: &[u8], emit: &mut impl FnMut(String)) {
        for &byte in chunk {
            match byte {
                b'\n' => {
                    self.pending_cr = false;
                    self.emit_all(emit);
                }
                b'\r' => {
                    self.resolve_bare_cr(emit);
                    self.pending_cr = true;
                }
                other => {
                    self.resolve_bare_cr(emit);
                    self.pending.push(other);
                    if self.pending.len() >= MAX_LINE_BYTES {
                        self.emit_chunk(emit);
                    }
                }
            }
        }
    }

    fn finish(&mut self, emit: &mut impl FnMut(String)) {
        self.resolve_bare_cr(emit);
        if !self.pending.is_empty() {
            self.emit_all(emit);
        }
    }
}

/// Delivers a run's updates and guarantees nothing is sent after `Finished`: the sender is
/// dropped under the same lock that publishes the final update.
struct Gate {
    sender: Mutex<Option<SyncSender<TaskUpdate>>>,
}

impl Gate {
    fn lock(&self) -> MutexGuard<'_, Option<SyncSender<TaskUpdate>>> {
        self.sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Blocks while the channel is full (backpressure); ignored once finished.
    fn send(&self, update: TaskUpdate) {
        if let Some(sender) = self.lock().as_ref() {
            let _ = sender.send(update);
        }
    }

    /// Publish the final update and close the gate for good.
    fn finish(&self, update: TaskUpdate) {
        if let Some(sender) = self.lock().take() {
            let _ = sender.send(update);
        }
    }
}

fn forward_lines(
    mut reader: impl Read,
    stream: OutputStream,
    run_id: String,
    terminal_id: String,
    gate: Arc<Gate>,
    done: Sender<()>,
) {
    let mut splitter = LineSplitter::default();
    let mut buffer = [0u8; 8192];
    let mut emit = |line: String| {
        gate.send(TaskUpdate::Output {
            run_id: run_id.clone(),
            terminal_id: terminal_id.clone(),
            stream,
            line,
        });
    };
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => splitter.push(&buffer[..read], &mut emit),
        }
    }
    splitter.finish(&mut emit);
    // Once the gate is closed `emit` is a no-op, but the loop above keeps reading until EOF
    // so a lingering background process never sees a broken pipe.
    let _ = done.send(());
}

/// What the caller tells the supervisor thread after trying to spawn the process.
enum Launch {
    Spawned {
        child: Child,
        stdout: Option<ChildStdout>,
        stderr: Option<ChildStderr>,
        active: Arc<ActiveRun>,
    },
    Failed(String),
}

impl TaskService {
    /// Create a service that reports through `updates`.
    ///
    /// The channel is bounded, which bounds memory: when the receiver falls behind, the
    /// output reader threads block, the child's pipe fills up and the child itself blocks
    /// on its next write until the receiver catches up. `run_task` never blocks on the
    /// channel (updates are sent from per-run supervisor threads), so the receiver may be
    /// drained from the same thread that calls `run_task`. Updates are dropped silently if
    /// the receiver is gone.
    pub fn new(updates: SyncSender<TaskUpdate>) -> Self {
        Self {
            inner: Arc::new(Inner {
                registry: Mutex::new(Registry::default()),
                updates,
            }),
        }
    }

    /// Convenience constructor returning the receiving end of a channel with
    /// [`UPDATE_CHANNEL_CAPACITY`] slots.
    pub fn channel() -> (Self, Receiver<TaskUpdate>) {
        Self::channel_with_capacity(UPDATE_CHANNEL_CAPACITY)
    }

    pub fn channel_with_capacity(capacity: usize) -> (Self, Receiver<TaskUpdate>) {
        let (tx, rx) = mpsc::sync_channel(capacity);
        (Self::new(tx), rx)
    }

    /// Port of `listTaskDefinitions`.
    pub fn list_task_definitions(&self, root_path: &Path) -> Vec<TaskDefinition> {
        discover_package_script_tasks(root_path)
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.inner
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Ids of the runs that are currently active.
    pub fn active_run_ids(&self) -> Vec<String> {
        self.registry().runs.keys().cloned().collect()
    }

    /// Port of `runTask`. Returns the initial run snapshot (status `running`); progress
    /// and the final result arrive as [`TaskUpdate`]s.
    ///
    /// A previous run on the same terminal is stopped (and its process tree gone) before
    /// the new process is spawned, so the two never overlap. `root_path` / the task cwd
    /// must be absolute. If the process cannot be spawned the run is reported as failed
    /// through the channel (like the TS `error` event) and the snapshot is still returned.
    /// `Err` is only returned when nothing was started.
    pub fn run_task(&self, root_path: &Path, request: &TaskRunRequest) -> Result<TaskRun> {
        let task = &request.task;
        let cwd = task.cwd.clone().unwrap_or_else(|| root_path.to_path_buf());
        if !cwd.is_absolute() {
            return Err(TaskError::NotAbsolute(cwd));
        }

        let previous = self
            .registry()
            .run_by_terminal
            .get(&request.terminal_id)
            .cloned();
        if let Some(previous_run_id) = previous {
            // Inline on purpose: the old process tree must be gone before the new one
            // starts (shared ports, lock files, ...).
            let _ = self.stop_task(&previous_run_id);
        }

        let run = TaskRun {
            run_id: format!("task-run-{}-{}", now_millis(), random_suffix()),
            task_id: task.id.clone(),
            label: task.label.clone(),
            command: task.command_line(),
            source: task.source,
            terminal_id: request.terminal_id.clone(),
            cwd: cwd.clone(),
            status: TaskRunStatus::Running,
            started_at: now_iso(),
            finished_at: None,
            exit_code: None,
            detail: task.detail.clone(),
        };

        // Start the supervisor first: if the OS refuses a thread, nothing has been spawned
        // yet and nothing can leak. It waits for the launch result below.
        let (launch_tx, launch_rx) = mpsc::channel();
        let gate = Arc::new(Gate {
            sender: Mutex::new(Some(self.inner.updates.clone())),
        });
        {
            let service = self.clone();
            let run = run.clone();
            thread::Builder::new()
                .name(format!("task-{}", run.run_id))
                .spawn(move || service.supervise(run, gate, launch_rx))
                .map_err(TaskError::Spawn)?;
        }

        let mut child = match build_command(task, &cwd).spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = launch_tx.send(Launch::Failed(error.to_string()));
                return Ok(run);
            }
        };

        let active = Arc::new(ActiveRun {
            pid: child.id(),
            stopped_by_user: AtomicBool::new(false),
            exit: ExitSignal::default(),
        });
        {
            let mut registry = self.registry();
            registry.runs.insert(run.run_id.clone(), Arc::clone(&active));
            registry
                .run_by_terminal
                .insert(run.terminal_id.clone(), run.run_id.clone());
        }

        let launch = Launch::Spawned {
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
            active,
        };
        if let Err(mpsc::SendError(Launch::Spawned { mut child, .. })) = launch_tx.send(launch) {
            // The supervisor vanished: do not leave an unsupervised process behind.
            let _ = child.kill();
            let _ = child.wait();
            self.unregister(&run.run_id, &run.terminal_id);
        }

        Ok(run)
    }

    fn unregister(&self, run_id: &str, terminal_id: &str) {
        let mut registry = self.registry();
        registry.runs.remove(run_id);
        if registry.run_by_terminal.get(terminal_id).map(String::as_str) == Some(run_id) {
            registry.run_by_terminal.remove(terminal_id);
        }
    }

    fn finish(&self, gate: &Gate, run: &TaskRun, status: TaskRunStatus, message: String, exit_code: Option<i32>) {
        self.unregister(&run.run_id, &run.terminal_id);
        let mut finished = run.clone();
        finished.status = status;
        finished.finished_at = Some(now_iso());
        finished.exit_code = exit_code;
        gate.finish(TaskUpdate::Finished {
            run: finished,
            message,
        });
    }

    fn supervise(&self, run: TaskRun, gate: Arc<Gate>, launch: Receiver<Launch>) {
        gate.send(TaskUpdate::Started {
            run: run.clone(),
            message: format!("{} started.", run.label),
        });

        let (mut child, stdout, stderr, active) = match launch.recv() {
            Ok(Launch::Spawned {
                child,
                stdout,
                stderr,
                active,
            }) => (child, stdout, stderr, active),
            Ok(Launch::Failed(message)) => {
                self.finish(&gate, &run, TaskRunStatus::Error, message, None);
                return;
            }
            Err(_) => {
                self.finish(&gate, &run, TaskRunStatus::Error, "Task launch was aborted.".into(), None);
                return;
            }
        };

        let (done_tx, done_rx) = mpsc::channel();
        let mut readers = 0;
        let mut reader_error = None;
        let mut start_reader = |name: &str, stream: OutputStream, source: Option<Box<dyn Read + Send>>| {
            let Some(source) = source else { return };
            let (gate, done) = (Arc::clone(&gate), done_tx.clone());
            let (run_id, terminal_id) = (run.run_id.clone(), run.terminal_id.clone());
            match thread::Builder::new()
                .name(format!("{name}-{}", run.run_id))
                .spawn(move || forward_lines(source, stream, run_id, terminal_id, gate, done))
            {
                Ok(_) => readers += 1,
                Err(error) => reader_error = Some(error.to_string()),
            }
        };
        start_reader("stdout", OutputStream::Stdout, stdout.map(|s| Box::new(s) as _));
        start_reader("stderr", OutputStream::Stderr, stderr.map(|s| Box::new(s) as _));
        drop(done_tx);

        if let Some(message) = reader_error {
            // Cannot observe the output: do not run unsupervised.
            let _ = terminate_tree(active.pid, &active.exit);
            let _ = child.kill();
            let _ = child.wait();
            active.exit.mark_exited();
            self.finish(
                &gate,
                &run,
                TaskRunStatus::Error,
                format!("Unable to read task output: {message}"),
                None,
            );
            return;
        }

        let wait_result = child.wait();
        active.exit.mark_exited();

        // Like Node's `close` event: wait for the pipes to drain, but not forever.
        let mut drained = 0;
        let deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
        while drained < readers {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if done_rx.recv_timeout(remaining).is_err() {
                break;
            }
            drained += 1;
        }
        let stopped = active.stopped_by_user.load(Ordering::SeqCst);
        if drained < readers && stopped {
            // The user asked for the end of the task: nothing may outlive it.
            force_kill_group(active.pid);
        }
        // Otherwise a background process was left running on purpose; the readers stay
        // detached, discard what it writes (the gate closes below) and exit at EOF.

        let (status, message, exit_code) = match wait_result {
            Err(error) => (TaskRunStatus::Error, error.to_string(), None),
            Ok(exit_status) => {
                let exit_code = exit_status.code();
                if stopped {
                    (
                        TaskRunStatus::Cancelled,
                        format!("{} was cancelled.", run.label),
                        exit_code,
                    )
                } else if exit_code == Some(0) {
                    (
                        TaskRunStatus::Success,
                        format!("{} finished successfully.", run.label),
                        exit_code,
                    )
                } else {
                    let shown = exit_code.map_or_else(|| "null".to_string(), |c| c.to_string());
                    (
                        TaskRunStatus::Error,
                        format!("{} exited with code {shown}.", run.label),
                        exit_code,
                    )
                }
            }
        };
        self.finish(&gate, &run, status, message, exit_code);
    }

    /// Port of `stopTask`: terminate the whole process tree of a run (also the background
    /// processes left behind in its group if the main process already exited). Blocks for
    /// up to a couple of seconds while waiting for the processes to go away. The run then
    /// ends with status `cancelled`.
    pub fn stop_task(&self, run_id: &str) -> Result<()> {
        let active = self
            .registry()
            .runs
            .get(run_id)
            .cloned()
            .ok_or(TaskError::NotRunning)?;

        active.stopped_by_user.store(true, Ordering::SeqCst);
        terminate_tree(active.pid, &active.exit)
    }

    /// Stop every active run (e.g. on shutdown). Returns the first error, if any.
    pub fn stop_all(&self) -> Result<()> {
        let mut first_error = None;
        for run_id in self.active_run_ids() {
            match self.stop_task(&run_id) {
                Ok(()) | Err(TaskError::NotRunning) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// Port of the request sanitising in `main/ipc/tasks.ts`: the task cwd must stay inside
/// the workspace (it defaults to the workspace root). The root must be absolute; relative
/// `cwd`s are resolved against it.
pub fn sanitize_task_request(root_path: &Path, request: &TaskRunRequest) -> Result<TaskRunRequest> {
    if !root_path.is_absolute() {
        return Err(TaskError::NotAbsolute(root_path.to_path_buf()));
    }
    let root = crate::paths::normalize_lexical(root_path);
    let cwd = match &request.task.cwd {
        Some(cwd) => {
            let resolved = crate::paths::try_resolve(&root, cwd)?;
            if !crate::paths::is_within(&root, &resolved) {
                return Err(TaskError::OutsideWorkspace(resolved));
            }
            resolved
        }
        None => root,
    };

    let mut sanitized = request.clone();
    sanitized.task.cwd = Some(cwd);
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(chunks: &[&[u8]]) -> Vec<String> {
        let mut splitter = LineSplitter::default();
        let mut lines = Vec::new();
        let mut emit = |line: String| lines.push(line);
        for chunk in chunks {
            splitter.push(chunk, &mut emit);
        }
        splitter.finish(&mut emit);
        lines
    }

    #[test]
    fn splits_on_lf_crlf_and_flushes_the_tail() {
        assert_eq!(split(&[b"a\nb\r\nc"]), ["a", "b", "c"]);
        assert_eq!(split(&[b"a\n\nb\n"]), ["a", "", "b"]);
        assert_eq!(split(&[b"a\r\n\r\nb\r\n"]), ["a", "", "b"]);
        assert!(split(&[b""]).is_empty());
    }

    #[test]
    fn bare_carriage_returns_flush_progress_output() {
        assert_eq!(split(&[b"10%\r20%\r30%\ndone"]), ["10%", "20%", "30%", "done"]);
        assert_eq!(split(&[b"x\r"]), ["x"]);
    }

    #[test]
    fn crlf_split_across_chunks_is_one_terminator() {
        assert_eq!(split(&[b"a\r", b"\nb\r", b"\n"]), ["a", "b"]);
        assert_eq!(split(&[b"a\r", b"b\n"]), ["a", "b"]);
    }

    #[test]
    fn long_lines_are_split_at_the_limit_without_breaking_utf8() {
        let long = "é".repeat(MAX_LINE_BYTES); // 2 bytes each
        let lines = split(&[long.as_bytes(), b"\n"]);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| l.len() <= MAX_LINE_BYTES && !l.contains('\u{fffd}')));
        assert_eq!(lines.concat(), long);

        let ascii = "x".repeat(MAX_LINE_BYTES * 2 + 5);
        let lines = split(&[ascii.as_bytes()]);
        assert_eq!(lines.iter().map(String::len).collect::<Vec<_>>(), [MAX_LINE_BYTES, MAX_LINE_BYTES, 5]);
    }
}
