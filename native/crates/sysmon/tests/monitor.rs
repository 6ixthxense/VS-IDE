//! Behavioural tests of the sampler and the pausable monitor thread.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use vsm_sysmon::{Collector, CollectorConfig, MonitorConfig, SysMonitor, SysStats};

fn fast_config() -> MonitorConfig {
    MonitorConfig {
        interval: Duration::from_millis(40),
        slow_interval: Duration::from_millis(500),
        start_paused: false,
    }
}

fn assert_shape(stats: &SysStats) {
    let cpu: f64 = stats.cpu.parse().expect("cpu is numeric text");
    assert!((0.0..=100.0001).contains(&cpu), "cpu {cpu}");
    assert!(stats.cpu.contains('.'));
    let ram: f64 = stats.ram.parse().expect("ram is numeric text");
    assert!(ram > 0.0 && ram <= 100.0, "ram {ram}");
    assert!(stats.ram_text.ends_with(" GB") && stats.ram_text.contains(" / "));
    assert!(!stats.gpu_name.is_empty());
    assert!(stats.processes.len() <= 5);
    let mut ids = std::collections::HashSet::new();
    for disk in &stats.disks {
        assert!(ids.insert(disk.id.clone()), "duplicate disk id {}", disk.id);
        assert!(!disk.name.is_empty());
    }
}

#[test]
fn collector_produces_a_complete_sample() {
    let mut collector = Collector::new(CollectorConfig::default());
    std::thread::sleep(Collector::MIN_CPU_INTERVAL + Duration::from_millis(50));
    let stats = collector.collect();
    assert_shape(&stats);
    // The first collect always performs the slow refresh.
    assert!(!stats.processes.is_empty(), "process list should not be empty");
    // JSON shape matches the TS `SysStats` field names.
    let json = serde_json::to_value(&stats).expect("json");
    for key in ["cpu", "ram", "ramText", "gpuName", "gpu", "disks", "processes"] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
}

#[test]
fn slow_data_is_cached_between_slow_refreshes() {
    let mut collector = Collector::new(CollectorConfig { slow_interval: Duration::from_secs(3600) });
    std::thread::sleep(Collector::MIN_CPU_INTERVAL + Duration::from_millis(50));
    let first = collector.collect();
    let second = collector.collect();
    assert_eq!(first.disks, second.disks);
    assert_eq!(first.processes, second.processes);
}

#[test]
fn monitor_delivers_samples_and_stops() {
    let (tx, rx) = mpsc::channel();
    let monitor = SysMonitor::start_with(fast_config(), move |stats| {
        let _ = tx.send(stats);
    })
    .expect("start");

    let first = rx.recv_timeout(Duration::from_secs(5)).expect("first sample");
    assert_shape(&first);
    for _ in 0..3 {
        let sample = rx.recv_timeout(Duration::from_secs(5)).expect("more samples");
        assert_shape(&sample);
    }
    monitor.stop();
    monitor.stop(); // idempotent
    while rx.try_recv().is_ok() {}
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "no samples after stop");
}

#[test]
fn paused_monitor_does_no_work_and_resume_restarts_it() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    let monitor = SysMonitor::start_with(fast_config(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .expect("start");

    wait_for(|| count.load(Ordering::SeqCst) >= 2);
    monitor.pause();
    // Let any in-flight sample finish, then verify nothing else arrives.
    std::thread::sleep(Duration::from_millis(300));
    let frozen = count.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(count.load(Ordering::SeqCst), frozen, "samples were produced while paused");

    monitor.resume();
    wait_for(|| count.load(Ordering::SeqCst) >= frozen + 2);
    monitor.pause();
    monitor.pause(); // idempotent
    monitor.resume();
    monitor.resume();
    drop(monitor); // Drop joins the thread
}

#[test]
fn start_paused_never_samples_until_resumed() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    let monitor = SysMonitor::start_with(
        MonitorConfig { start_paused: true, ..fast_config() },
        move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        },
    )
    .expect("start");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    monitor.resume();
    wait_for(|| count.load(Ordering::SeqCst) >= 1);
    monitor.set_interval(Duration::from_millis(20));
    wait_for(|| count.load(Ordering::SeqCst) >= 5);
}

#[test]
fn paused_thread_uses_no_cpu() {
    // Process CPU time is only cheaply readable on Linux; elsewhere the channel-parking design
    // is covered by `paused_monitor_does_no_work_and_resume_restarts_it`.
    #[cfg(target_os = "linux")]
    {
        fn cpu_ticks() -> u64 {
            let stat = std::fs::read_to_string("/proc/self/stat").expect("stat");
            let after = &stat[stat.rfind(')').expect("paren") + 2..];
            let fields: Vec<&str> = after.split_whitespace().collect();
            fields[11].parse::<u64>().expect("utime") + fields[12].parse::<u64>().expect("stime")
        }
        let monitor = SysMonitor::start_with(
            MonitorConfig { interval: Duration::from_millis(20), ..fast_config() },
            |_| {},
        )
        .expect("start");
        std::thread::sleep(Duration::from_millis(400));
        monitor.pause();
        std::thread::sleep(Duration::from_millis(200));
        let before = cpu_ticks();
        std::thread::sleep(Duration::from_secs(2));
        let used = cpu_ticks() - before;
        // other test threads may run in parallel; the monitor thread itself must be idle.
        assert!(used < 50, "unexpected CPU use while paused: {used} ticks");
        monitor.stop();
    }
}

#[test]
fn drop_stops_the_thread() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    let monitor = SysMonitor::start_with(fast_config(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .expect("start");
    wait_for(|| count.load(Ordering::SeqCst) >= 1);
    drop(monitor);
    let after_drop = count.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(count.load(Ordering::SeqCst), after_drop);
}

#[test]
fn dropping_the_monitor_inside_its_own_callback_does_not_deadlock() {
    use std::sync::Mutex;

    let slot: Arc<Mutex<Option<SysMonitor>>> = Arc::new(Mutex::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    let (done_tx, done_rx) = mpsc::channel();
    let monitor = {
        let slot = Arc::clone(&slot);
        let calls = Arc::clone(&calls);
        SysMonitor::start_with(fast_config(), move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            // Take the monitor out of the shared slot and drop it on the monitor thread
            // (Drop -> stop -> would join itself without the fix).
            let taken = slot.lock().expect("slot").take();
            drop(taken);
            let _ = done_tx.send(());
        })
        .expect("start")
    };
    *slot.lock().expect("slot") = Some(monitor);

    done_rx.recv_timeout(Duration::from_secs(5)).expect("callback finished (no self-join deadlock)");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no samples after the in-callback drop");
}

#[test]
fn explicit_stop_inside_the_callback_is_safe() {
    use std::sync::Mutex;

    let slot: Arc<Mutex<Option<Arc<SysMonitor>>>> = Arc::new(Mutex::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    let monitor = {
        let slot = Arc::clone(&slot);
        let calls = Arc::clone(&calls);
        Arc::new(
            SysMonitor::start_with(fast_config(), move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                let current = slot.lock().expect("slot").clone();
                if let Some(m) = current {
                    m.stop();
                }
            })
            .expect("start"),
        )
    };
    *slot.lock().expect("slot") = Some(Arc::clone(&monitor));
    wait_for(|| calls.load(Ordering::SeqCst) >= 1);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    *slot.lock().expect("slot") = None; // break the Arc cycle
}

#[test]
fn set_interval_interrupts_the_current_wait() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    let monitor = SysMonitor::start_with(
        MonitorConfig { interval: Duration::from_secs(120), ..fast_config() },
        move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        },
    )
    .expect("start");
    wait_for(|| count.load(Ordering::SeqCst) >= 1);
    // The monitor is now waiting ~120 s for the next tick; shrinking the interval must take effect now.
    let started = Instant::now();
    monitor.set_interval(Duration::from_millis(30));
    wait_for(|| count.load(Ordering::SeqCst) >= 3);
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
}

fn wait_for(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for condition");
        std::thread::sleep(Duration::from_millis(10));
    }
}
