//! Memory and processor use of the IDE itself, for the status bar. A thread of its own looks at
//! the process once a second, so that the interface never waits for the system.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};

const EVERY: Duration = Duration::from_secs(1);

/// Resident memory of the process in bytes, and its processor use in tenths of a percent of the
/// whole processor (what the task manager shows, not of one core).
static MEMORY: AtomicU64 = AtomicU64::new(0);
static CPU_TENTHS: AtomicU64 = AtomicU64::new(0);

/// A process tree (a terminal's shell and what runs in it) someone asked about lately.
struct Watched {
    asked: Instant,
    usage: Option<(u64, f32)>,
}

static WATCHED: Mutex<Option<HashMap<u32, Watched>>> = Mutex::new(None);
/// A tree nobody asks about for this long is no longer looked at.
const FORGET_AFTER: Duration = Duration::from_secs(5);

/// The memory (bytes) and processor use (percent of the whole processor) of the process `pid`
/// and everything it started, as of the last second. The first call starts watching it, so it
/// answers None until the next look. Ask every frame you show it: that is what keeps it watched.
pub fn watch(pid: u32) -> Option<(u64, f32)> {
    start();
    let mut guard = WATCHED.lock().unwrap_or_else(|e| e.into_inner());
    let entry = guard.get_or_insert_with(HashMap::new).entry(pid).or_insert(Watched { asked: Instant::now(), usage: None });
    entry.asked = Instant::now();
    entry.usage
}

/// What `root` and all its descendants use, from the usage of each process and who started whom.
pub fn tree_total(children: &HashMap<u32, Vec<u32>>, usage: &HashMap<u32, (u64, f32)>, root: u32) -> (u64, f32) {
    let (mut memory, mut cpu) = (0, 0.0);
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        // A pid reused by the system could make a loop.
        if !seen.insert(pid) {
            continue;
        }
        if let Some((m, c)) = usage.get(&pid) {
            memory += m;
            cpu += c;
        }
        stack.extend(children.get(&pid).into_iter().flatten().copied());
    }
    (memory, cpu)
}

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
            let wanted: Vec<u32> = {
                let mut guard = WATCHED.lock().unwrap_or_else(|e| e.into_inner());
                let map = guard.get_or_insert_with(HashMap::new);
                map.retain(|_, w| w.asked.elapsed() < FORGET_AFTER);
                map.keys().copied().collect()
            };
            // The processor use is a difference between two looks: the first one reads 0.
            if wanted.is_empty() {
                system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            } else {
                // Terminals: every process is needed to know who started whom.
                system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
            }
            if let Some(process) = system.process(pid) {
                MEMORY.store(process.memory(), Ordering::Relaxed);
                CPU_TENTHS.store((process.cpu_usage() / cores * 10.0).max(0.0) as u64, Ordering::Relaxed);
            }
            if !wanted.is_empty() {
                let mut usage: HashMap<u32, (u64, f32)> = HashMap::new();
                let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
                for (child, process) in system.processes() {
                    usage.insert(child.as_u32(), (process.memory(), process.cpu_usage() / cores));
                    if let Some(parent) = process.parent() {
                        children.entry(parent.as_u32()).or_default().push(child.as_u32());
                    }
                }
                let mut guard = WATCHED.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(map) = guard.as_mut() {
                    for root in wanted {
                        if let Some(entry) = map.get_mut(&root) {
                            entry.usage = Some(tree_total(&children, &usage, root));
                        }
                    }
                }
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
    if mo >= 1024.0 {
        format!("{:.1} Go", mo / 1024.0).replace('.', ",")
    } else {
        format!("{mo:.0} Mo")
    }
}

/// `3,0 %`, `12 %`.
fn format_cpu(cpu: f32) -> String {
    if cpu < 10.0 { format!("{cpu:.1} %").replace('.', ",") } else { format!("{cpu:.0} %") }
}

/// `312 Mo · 3,0 %`: what a terminal shows in its title.
pub fn format_usage(bytes: u64, cpu: f32) -> String {
    format!("{} · {}", format_memory(bytes), format_cpu(cpu))
}

/// The widest `format_usage` shows: the room to keep for it, so that it never moves what is next to it.
pub const WIDEST_USAGE: &str = "9999 Mo · 99,9 %";

/// What the status bar shows: `RAM 312 Mo · CPU 3 %`.
pub fn summary() -> Option<String> {
    let bytes = memory_bytes();
    (bytes > 0).then(|| format!("RAM {} · CPU {}", format_memory(bytes), format_cpu(cpu_percent())))
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
        assert!(
            memory_bytes() > 1024 * 1024,
            "this test process uses more than a megabyte: {}",
            memory_bytes()
        );
        let summary = summary().expect("a summary once the memory is known");
        assert!(
            summary.starts_with("RAM ") && summary.contains("CPU ") && summary.ends_with('%'),
            "{summary}"
        );
        assert!(cpu_percent() >= 0.0 && cpu_percent() <= 100.0 + 1.0);
    }

    #[test]
    fn a_tree_adds_the_process_and_all_that_it_started() {
        let usage: HashMap<u32, (u64, f32)> = [(1, (100, 1.0)), (2, (50, 2.0)), (3, (25, 0.5)), (9, (1000, 9.0))].into();
        let children: HashMap<u32, Vec<u32>> = [(1, vec![2]), (2, vec![3]), (3, vec![1])].into();
        assert_eq!(tree_total(&children, &usage, 1), (175, 3.5), "a loop (a reused pid) is counted once, other trees are not");
        assert_eq!(tree_total(&children, &usage, 3), (175, 3.5));
        assert_eq!(tree_total(&children, &usage, 77), (0, 0.0));
        assert_eq!(format_usage(312 * 1024 * 1024, 0.31), "312 Mo · 0,3 %");
        assert_eq!(format_usage(2048 * 1024 * 1024, 42.0), "2,0 Go · 42 %");
    }

    #[test]
    fn what_a_process_started_is_counted_with_it() {
        // A child that sleeps for a few seconds, started by this test process.
        #[cfg(windows)]
        let mut child = std::process::Command::new("ping").args(["-n", "60", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        #[cfg(not(windows))]
        let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
        let mut found = None;
        // A busy machine (the other tests run side by side) may take long to look at every process.
        for _ in 0..1600 {
            found = watch(child.id());
            if found.is_some_and(|(memory, _)| memory > 0) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.kill();
        let (memory, cpu) = found.expect("the tree is known within a few seconds");
        assert!(memory > 100 * 1024, "a process uses some memory: {memory}");
        assert!((0.0..=100.0).contains(&cpu));
    }
}
