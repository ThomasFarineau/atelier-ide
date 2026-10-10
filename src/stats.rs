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
    let entry = guard
        .get_or_insert_with(HashMap::new)
        .entry(pid)
        .or_insert(Watched {
            asked: Instant::now(),
            usage: None,
        });
    entry.asked = Instant::now();
    entry.usage
}

/// What `root` and all its descendants use, from the usage of each process and who started whom.
pub fn tree_total(
    children: &HashMap<u32, Vec<u32>>,
    usage: &HashMap<u32, (u64, f32)>,
    root: u32,
) -> (u64, f32) {
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
    let _ = std::thread::Builder::new()
        .name("stats".into())
        .spawn(move || {
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
                    CPU_TENTHS.store(
                        (process.cpu_usage() / cores * 10.0).max(0.0) as u64,
                        Ordering::Relaxed,
                    );
                }
                if !wanted.is_empty() {
                    let mut usage: HashMap<u32, (u64, f32)> = HashMap::new();
                    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
                    for (child, process) in system.processes() {
                        usage.insert(
                            child.as_u32(),
                            (process.memory(), process.cpu_usage() / cores),
                        );
                        if let Some(parent) = process.parent() {
                            children
                                .entry(parent.as_u32())
                                .or_default()
                                .push(child.as_u32());
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
        decimal(trf!("gb", mo = format!("{:.1}", mo / 1024.0)))
    } else {
        trf!("mb", mo = format!("{:.0}", mo))
    }
}

/// `3,0 %`, `12 %`.
fn format_cpu(cpu: f32) -> String {
    if cpu < 10.0 {
        decimal(format!("{cpu:.1} %"))
    } else {
        format!("{cpu:.0} %")
    }
}

/// The decimal separator of the language: a comma in French, a point in English.
fn decimal(text: String) -> String {
    text.replace('.', tr!("decimal_point"))
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
    (bytes > 0).then(|| {
        format!(
            "RAM {} · CPU {}",
            format_memory(bytes),
            format_cpu(cpu_percent())
        )
    })
}

#[cfg(test)]
#[path = "../test/stats/tests.rs"]
mod tests;
