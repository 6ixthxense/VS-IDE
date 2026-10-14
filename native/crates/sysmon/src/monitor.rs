//! Background polling loop (replaces `startSysMonitor` in `main.ts`).
//!
//! The worker thread blocks on an `mpsc` channel: while running it waits with
//! `recv_timeout(next tick)`, while paused it waits with a plain `recv()`, so a paused monitor
//! consumes no CPU at all (no sleep polling, no timers).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::collector::{Collector, CollectorConfig, DEFAULT_SLOW_INTERVAL};
use crate::stats::SysStats;
use crate::SysMonError;

/// Default CPU/RAM tick (2 s, as in the rewrite plan).
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// CPU + RAM (+ GPU) cadence.
    pub interval: Duration,
    /// Disk + process cadence.
    pub slow_interval: Duration,
    /// Spawn the worker parked; nothing is sampled until [`SysMonitor::resume`].
    pub start_paused: bool,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self { interval: DEFAULT_INTERVAL, slow_interval: DEFAULT_SLOW_INTERVAL, start_paused: false }
    }
}

enum Command {
    Pause,
    Resume,
    SetInterval(Duration),
    Stop,
}

/// Handle to the polling thread.  Dropping it stops and joins the thread.
pub struct SysMonitor {
    tx: Sender<Command>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

struct Worker<F: FnMut(SysStats)> {
    rx: Receiver<Command>,
    callback: F,
    interval: Duration,
    slow_interval: Duration,
    running: bool,
    stopped: bool,
}

impl<F: FnMut(SysStats)> Worker<F> {
    fn apply(&mut self, command: Command) {
        match command {
            Command::Pause => self.running = false,
            Command::Resume => self.running = true,
            Command::SetInterval(d) => self.interval = d,
            Command::Stop => self.stopped = true,
        }
    }

    /// Block until the next tick (`started + interval`, recomputed when the interval changes) or
    /// until `fixed` when given.  Returns early on `pause`/`stop`.
    fn wait_until(&mut self, started: Instant, fixed: Option<Instant>) {
        while self.running && !self.stopped {
            let deadline = fixed.unwrap_or(started + self.interval);
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            match self.rx.recv_timeout(deadline - now) {
                // `SetInterval` just updates `self.interval`; the loop recomputes the deadline.
                Ok(command) => self.apply(command),
                Err(RecvTimeoutError::Timeout) => return,
                Err(RecvTimeoutError::Disconnected) => self.stopped = true,
            }
        }
    }

    fn run(mut self) {
        let mut collector: Option<Collector> = None;
        let mut needs_warmup = true;
        while !self.stopped {
            if !self.running {
                // Paused: park until a command arrives.  Zero work.
                needs_warmup = true;
                match self.rx.recv() {
                    Ok(command) => self.apply(command),
                    Err(_) => self.stopped = true,
                }
                continue;
            }

            if needs_warmup {
                match collector.as_mut() {
                    Some(c) => c.prime(),
                    None => {
                        collector = Some(Collector::new(CollectorConfig { slow_interval: self.slow_interval }));
                    }
                }
                needs_warmup = false;
                // CPU usage is a delta; give sysinfo its minimum window before the first read.
                let now = Instant::now();
                self.wait_until(now, Some(now + Collector::MIN_CPU_INTERVAL + Duration::from_millis(25)));
                if !self.running || self.stopped {
                    continue;
                }
            }

            if let Some(c) = collector.as_mut() {
                (self.callback)(c.collect());
            }
            self.wait_until(Instant::now(), None);
        }
    }
}

impl SysMonitor {
    /// Start polling every `interval` (disks/processes every 15 s) and call `callback` on the
    /// monitor thread with each sample.
    pub fn start<F>(interval: Duration, callback: F) -> Result<Self, SysMonError>
    where
        F: FnMut(SysStats) + Send + 'static,
    {
        Self::start_with(MonitorConfig { interval, ..MonitorConfig::default() }, callback)
    }

    pub fn start_with<F>(config: MonitorConfig, callback: F) -> Result<Self, SysMonError>
    where
        F: FnMut(SysStats) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let worker = Worker {
            rx,
            callback,
            interval: config.interval.max(Duration::from_millis(10)),
            slow_interval: config.slow_interval,
            running: !config.start_paused,
            stopped: false,
        };
        let handle = std::thread::Builder::new()
            .name("vsm-sysmon".into())
            .spawn(move || worker.run())?;
        Ok(Self { tx, handle: Mutex::new(Some(handle)) })
    }

    /// Stop sampling.  The thread parks on its channel and does nothing until [`Self::resume`].
    pub fn pause(&self) {
        let _ = self.tx.send(Command::Pause);
    }

    /// Resume sampling; a fresh sample is delivered after a short CPU warm-up window.
    pub fn resume(&self) {
        let _ = self.tx.send(Command::Resume);
    }

    /// Change the CPU/RAM tick.
    pub fn set_interval(&self, interval: Duration) {
        let _ = self.tx.send(Command::SetInterval(interval.max(Duration::from_millis(10))));
    }

    /// Stop and join the thread.  Idempotent.  Safe to call from inside the callback (i.e. on
    /// the monitor thread itself): there the thread cannot join itself, so it is told to stop
    /// and detached; it exits as soon as the callback returns.
    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
        let handle = self.handle.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(handle) = handle {
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for SysMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}
