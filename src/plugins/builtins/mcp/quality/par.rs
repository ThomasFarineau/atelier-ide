//! Running work on several threads.

use super::args::Args;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One thread per core.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The `--jobs` option: how many threads to use.
pub fn jobs(args: &Args) -> Result<usize, String> {
    match args.number("jobs", default_jobs())? {
        0 => Err("--jobs must be at least 1".into()),
        n => Ok(n),
    }
}

/// Map `f` over `items` on up to `jobs` threads, keeping the order of `items`.
pub fn map<T: Sync, R: Send>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(item) = items.get(i) else { break };
                    let result = f(item);
                    *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
                }
            });
        }
    });
    slots
        .into_iter()
        .filter_map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .collect()
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/par/tests.rs"]
mod tests;
