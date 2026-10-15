//! Inspect Code: the syntax errors of every source file of the project, found in the background
//! and listed in the Problems window next to those of the open documents; and the same check on
//! a few files, for the commit window.

use super::{Ide, file_index};
use crate::{
    languages::{Diagnostic, Severity, inspect},
    plugins::builtin::Problem,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

/// Problems kept at most, so that a broken project does not fill the memory.
const MAX_PROBLEMS: usize = 5_000;

/// The inspection of the project in progress or done.
pub struct Inspection {
    rx: Option<Receiver<Update>>,
    pub problems: Vec<Problem>,
    pub files: usize,
    pub total: usize,
}

enum Update {
    Total(usize),
    File(Vec<Problem>),
    Done,
}

/// A problem of a file, from a diagnostic with byte offsets in `text`.
pub fn problem_of(path: &Path, text: &str, d: &Diagnostic) -> Problem {
    let start = d.range.start.min(text.len());
    let end = d.range.end.min(text.len()).max(start);
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    Problem {
        path: path.to_path_buf(),
        line: text[..start].matches('\n').count(),
        column: text[line_start..start].chars().count(),
        length: text[start..end].chars().count().max(1),
        severity: match d.severity {
            Severity::Error => 1,
            Severity::Warning => 2,
            Severity::Info => 3,
            Severity::Hint => 4,
        },
        message: d.message.clone(),
        source: d.source.clone(),
    }
}

/// The problems of a file read from the disk.
pub fn inspect_file(path: &Path) -> Vec<Problem> {
    if inspect::language_of(path).is_none() {
        return Vec::new();
    }
    let small = fs::metadata(path).is_ok_and(|m| m.len() <= inspect::MAX_SIZE);
    let Some(text) = small.then(|| fs::read_to_string(path).ok()).flatten() else {
        return Vec::new();
    };
    inspect::syntax_errors(path, &text)
        .iter()
        .map(|d| problem_of(path, &text, d))
        .collect()
}

impl Ide {
    /// Starts inspecting the project, or starts it again.
    pub(super) fn start_inspection(&mut self) {
        let (tx, rx) = mpsc::channel();
        let root = self.root.clone();
        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            let files: Vec<PathBuf> = file_index::list(&root)
                .into_iter()
                .filter(|p| inspect::language_of(p).is_some())
                .collect();
            let _ = tx.send(Update::Total(files.len()));
            for path in files {
                if tx.send(Update::File(inspect_file(&path))).is_err() {
                    return;
                }
                repaint();
            }
            let _ = tx.send(Update::Done);
            repaint();
        });
        self.inspection = Some(Inspection {
            rx: Some(rx),
            problems: Vec::new(),
            files: 0,
            total: 0,
        });
        self.status = tr!("inspection_started").into();
    }

    /// Takes what the inspection found since the last frame.
    pub(super) fn poll_inspection(&mut self) {
        let Some(inspection) = &mut self.inspection else {
            return;
        };
        let Some(rx) = &inspection.rx else { return };
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(Update::Total(n)) => inspection.total = n,
                Ok(Update::File(found)) => {
                    inspection.files += 1;
                    let room = MAX_PROBLEMS.saturating_sub(inspection.problems.len());
                    inspection.problems.extend(found.into_iter().take(room));
                }
                Ok(Update::Done) | Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if finished {
            inspection.rx = None;
            let (errors, files) = (inspection.problems.len(), inspection.files);
            self.status = trf!("inspection_done", errors = errors, files = files);
        }
    }

    /// The files read so far and how many there are, while the inspection runs.
    pub(super) fn inspection_progress(&self) -> Option<(usize, usize)> {
        let inspection = self.inspection.as_ref()?;
        inspection
            .rx
            .is_some()
            .then_some((inspection.files, inspection.total))
    }
}

#[cfg(test)]
#[path = "../../test/app/inspect/tests.rs"]
mod tests;
