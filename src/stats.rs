//! Memory and processor use of the IDE itself, for the status bar. A thread of its own looks at
//! the process once a second, so that the interface never waits for the system.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};

const EVERY: Duration = Duration::from_secs(1);

/// Resident memory of the process in bytes, and its processor use in tenths of a percent of the
/// whole processor (what the task manager shows, not of one core).
static MEMORY: AtomicU64 = AtomicU64::new(0);
static CPU_TENTHS: AtomicU64 = AtomicU64::new(0);

/// Starts looking at the process. Once only, whoever asks.
pub fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let Ok(pid) = get_current_pid() else { return };
    let _ = std::thread::Builder::new().name("stats".into()).spawn(move || {
        let mut system = System::new();
        let kind = ProcessRefreshKind::nothing().with_memory().with_cpu();
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f32;
        loop {
            // The processor use is a difference between two looks: the first one reads 0.
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            if let Some(process) = system.process(pid) {
                MEMORY.store(process.memory(), Ordering::Relaxed);
                CPU_TENTHS.store((process.cpu_usage() / cores * 10.0).max(0.0) as u64, Ordering::Relaxed);
            }
            std::thread::sleep(EVERY);
        }
    });
}

pub fn memory_bytes() -> u64 {
    MEMORY.load(Ordering::Relaxed)
}

/// Percent of the whole processor.
pub fn cpu_percent() -> f32 {
    CPU_TENTHS.load(Ordering::Relaxed) as f32 / 10.0
}

/// `312 Mo`, `1,4 Go`.
pub fn format_memory(bytes: u64) -> String {
    const MO: f64 = 1024.0 * 1024.0;
    let mo = bytes as f64 / MO;
    if mo >= 1024.0 { format!("{:.1} Go", mo / 1024.0).replace('.', ",") } else { format!("{mo:.0} Mo") }
}

/// What the status bar shows: `RAM 312 Mo · CPU 3 %`.
pub fn summary() -> Option<String> {
    let bytes = memory_bytes();
    (bytes > 0).then(|| {
        let cpu = cpu_percent();
        format!("RAM {} · CPU {}", format_memory(bytes), if cpu < 10.0 { format!("{cpu:.1} %").replace('.', ",") } else { format!("{cpu:.0} %") })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_shown_in_mo_then_in_go() {
        assert_eq!(format_memory(312 * 1024 * 1024 + 1000), "312 Mo");
        assert_eq!(format_memory(1536 * 1024 * 1024), "1,5 Go");
        assert_eq!(format_memory(0), "0 Mo");
    }

    #[test]
    fn the_process_is_looked_at_and_its_memory_is_known_within_a_few_seconds() {
        start();
        start();
        for _ in 0..500 {
            if memory_bytes() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(memory_bytes() > 1024 * 1024, "this test process uses more than a megabyte: {}", memory_bytes());
        let summary = summary().expect("a summary once the memory is known");
        assert!(summary.starts_with("RAM ") && summary.contains("CPU ") && summary.ends_with('%'), "{summary}");
        assert!(cpu_percent() >= 0.0 && cpu_percent() <= 100.0 + 1.0);
    }
}
