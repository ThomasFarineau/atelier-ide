//! Background tasks, as IntelliJ lists them at the right of the status bar: long work (a diagram
//! to draw, an export) runs on a thread of its own, announces itself here and the window stays
//! alive while it runs.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

struct Entry {
    id: u64,
    title: String,
    started: Instant,
}

static TASKS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

/// What the interface shows of a running task.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub title: String,
    pub elapsed: Duration,
}

fn tasks() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    TASKS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A task that is running: it leaves the list when this is dropped, even if its thread panics.
pub struct Task(u64);

/// Announces a task. Keep the returned value for as long as it runs.
pub fn start(title: impl Into<String>) -> Task {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    tasks().push(Entry { id, title: title.into(), started: Instant::now() });
    Task(id)
}

impl Drop for Task {
    fn drop(&mut self) {
        tasks().retain(|e| e.id != self.0);
    }
}

/// The running tasks, the oldest first.
pub fn running() -> Vec<Info> {
    tasks()
        .iter()
        .map(|e| Info { title: e.title.clone(), elapsed: e.started.elapsed() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_is_listed_while_it_lives_even_after_a_panic() {
        let marker = format!("test-{}", NEXT.load(Ordering::Relaxed));
        let task = start(marker.clone());
        let mine = |list: Vec<Info>| list.into_iter().find(|i| i.title == marker);
        assert!(mine(running()).is_some());
        drop(task);
        assert!(mine(running()).is_none());
        let marker2 = format!("{marker}-panic");
        let name = marker2.clone();
        let _ = std::thread::spawn(move || {
            let _task = start(name);
            panic!("boom");
        })
        .join();
        assert!(running().iter().all(|i| i.title != marker2), "the guard went with the thread");
    }
}
