//! The quality run in progress, which the status bar shows and can pause.
//!
//! One run at a time: two would fight over the worker copies and the `.metrics/` snapshots. A
//! pause takes effect at the next step boundary (between gates, coverage commands and mutants):
//! the commands already running finish, nothing new starts until the run is resumed.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// What the status bar shows of the run.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    /// The tool called (`checker`, `mutator`…).
    pub tool: String,
    /// The gate running now.
    pub gate: String,
    /// What the gate is doing (a coverage command, the warm-up of the workers…).
    pub phase: String,
    /// Mutants tried and to try, once the mutator knows.
    pub done: usize,
    pub total: usize,
    pub started: Instant,
    /// Time spent paused, left out of `elapsed`.
    pub paused_for: Duration,
}

impl Progress {
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed().saturating_sub(self.paused_for)
    }
}

static RUN: Mutex<Option<Progress>> = Mutex::new(None);
static PAUSED: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut Option<Progress>) -> R) -> R {
    f(&mut RUN.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The run while it lasts; dropping it ends the run (and its pause).
pub struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        with(|run| *run = None);
        PAUSED.store(false, Ordering::SeqCst);
    }
}

/// Starts a run of `tool`, unless one is going on.
pub fn start(tool: &str) -> Result<Guard, String> {
    with(|run| {
        if let Some(other) = run {
            return Err(format!(
                "a quality run ({}) is already going on in this IDE: wait for it to end",
                other.tool
            ));
        }
        *run = Some(Progress {
            tool: tool.into(),
            gate: String::new(),
            phase: String::new(),
            done: 0,
            total: 0,
            started: Instant::now(),
            paused_for: Duration::ZERO,
        });
        PAUSED.store(false, Ordering::SeqCst);
        Ok(Guard(()))
    })
}

/// The run in progress.
pub fn current() -> Option<Progress> {
    with(|run| run.clone())
}

fn update(f: impl FnOnce(&mut Progress)) {
    with(|run| run.as_mut().map(f));
}

/// The gate starting now.
pub fn gate(name: &str) {
    update(|p| {
        p.gate = name.into();
        p.phase.clear();
        p.done = 0;
        p.total = 0;
    });
}

pub fn phase(text: &str) {
    update(|p| p.phase = text.into());
}

/// The number of mutants to try.
pub fn total(n: usize) {
    update(|p| {
        p.total = n;
        p.done = 0;
    });
}

/// One more mutant tried.
pub fn tick() {
    update(|p| p.done += 1);
}

pub fn paused() -> bool {
    PAUSED.load(Ordering::SeqCst) && current().is_some()
}

/// Pauses or resumes the run in progress.
pub fn set_paused(paused: bool) {
    if current().is_some() {
        PAUSED.store(paused, Ordering::SeqCst);
    }
}

/// A step boundary: waits here while the run is paused.
pub fn checkpoint() {
    let start = Instant::now();
    while paused() {
        std::thread::sleep(Duration::from_millis(100));
    }
    let waited = start.elapsed();
    if waited >= Duration::from_millis(100) {
        update(|p| p.paused_for += waited);
    }
}

/// The tests share one run state: they take this lock so as not to run two runs at once.
#[cfg(test)]
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/run/tests.rs"]
mod tests;
