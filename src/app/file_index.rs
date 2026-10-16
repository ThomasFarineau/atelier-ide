//! The files of the project for Search Everywhere. They are listed in the background, with their
//! names already in lower case, so that opening the popup and typing in it never walk the disk
//! nor build a string per file on the thread of the window.

use crate::project;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

/// The list is made again when the popup opens after this long.
const STALE: Duration = Duration::from_secs(20);
/// Files kept at most: a bigger project is searched through its first files only.
const MAX_FILES: usize = 20_000;

pub struct Entry {
    pub path: PathBuf,
    /// The file name in lower case, to compare with what is typed.
    pub name: String,
}

#[derive(Default)]
pub struct Index {
    pending: Option<mpsc::Receiver<Vec<Entry>>>,
    entries: Arc<Vec<Entry>>,
    built: Option<(PathBuf, Instant)>,
    /// Changes each time `entries` does: a search kept for a generation is still true.
    pub generation: u64,
}

impl Index {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The list is wrong (a file was created, renamed or deleted): it is made again at the next
    /// refresh.
    pub fn invalidate(&mut self) {
        self.built = None;
    }

    /// Lists the files of `root` again in the background, unless they were just listed. `wake` is
    /// called once they are.
    pub fn refresh(&mut self, root: &Path, wake: impl Fn() + Send + 'static) {
        let fresh = self
            .built
            .as_ref()
            .is_some_and(|(r, at)| r == root && at.elapsed() < STALE);
        if self.pending.is_some() || fresh {
            return;
        }
        if self.built.as_ref().is_some_and(|(r, _)| r != root) {
            self.entries = Arc::default();
            self.generation += 1;
        }
        self.built = Some((root.to_path_buf(), Instant::now()));
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let entries = list(&root)
                .into_iter()
                .map(|path| Entry {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase(),
                    path,
                })
                .collect();
            let _ = tx.send(entries);
            wake();
        });
    }

    /// Takes the list once it is made.
    pub fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(entries) => {
                self.entries = Arc::new(entries);
                self.generation += 1;
                self.pending = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

/// Project files: hidden folders, dependencies and build output are skipped.
pub fn list(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(children) = project::searchable_children(&dir) else {
            continue;
        };
        for child in children {
            if child.is_dir() {
                if !child.is_symlink() {
                    stack.push(child);
                }
            } else {
                files.push(child);
            }
            if files.len() >= MAX_FILES {
                return files;
            }
        }
    }
    files
}
