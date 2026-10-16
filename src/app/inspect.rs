//! Inspect Code: the syntax errors of every source file of the project, found in the background
//! and listed in the Problems window next to those of the open documents; and the same check on
//! a few files, for the commit window.

use super::{Ide, file_index};
use crate::{
    languages::{Diagnostic, Severity, inspect, lsp::ServerDiagnostic},
    plugins::builtin::Problem,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
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

/// A problem the language server found, as the Problems window lists it.
pub fn problem_of_server(path: &Path, d: &ServerDiagnostic) -> Problem {
    Problem {
        path: path.to_path_buf(),
        line: d.start.0,
        column: d.start.1,
        length: if d.end.0 == d.start.0 {
            d.end.1.saturating_sub(d.start.1).max(1)
        } else {
            1
        },
        severity: d.severity,
        message: d.message.clone(),
        source: d.source.clone(),
    }
}

/// A problem of the compiler of TypeScript.
fn problem_of_compiled(c: &inspect::Compiled) -> Problem {
    Problem {
        path: c.path.clone(),
        line: c.line,
        column: c.column,
        length: 1,
        severity: if c.error { 1 } else { 2 },
        message: c.message.clone(),
        source: "tsc".into(),
    }
}

/// The lines of a text that a merge left in it: `<<<<<<<`, `=======` and `>>>>>>>`.
pub fn conflict_markers(path: &Path, text: &str) -> Vec<Problem> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim_end();
            line == "======="
                || line == "<<<<<<<"
                || line == ">>>>>>>"
                || line.starts_with("<<<<<<< ")
                || line.starts_with(">>>>>>> ")
        })
        .map(|(n, line)| Problem {
            path: path.to_path_buf(),
            line: n,
            column: 0,
            length: line.chars().count().max(1),
            severity: 1,
            message: tr!("merge_conflict_marker").to_string(),
            source: "git".into(),
        })
        .collect()
}

/// The problems of a file read from the disk: its syntax errors, and the marks of a merge.
pub fn inspect_file(path: &Path) -> Vec<Problem> {
    let small = fs::metadata(path).is_ok_and(|m| m.len() <= inspect::MAX_SIZE);
    let Some(text) = small.then(|| fs::read_to_string(path).ok()).flatten() else {
        return Vec::new();
    };
    let mut found: Vec<Problem> = inspect::syntax_errors(path, &text)
        .iter()
        .map(|d| problem_of(path, &text, d))
        .collect();
    found.extend(conflict_markers(path, &text));
    found
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
            // The type errors of a TypeScript project, from its own compiler.
            let compiled: Vec<Problem> = inspect::run_tsc(&root, Duration::from_secs(120))
                .iter()
                .map(problem_of_compiled)
                .collect();
            let _ = tx.send(Update::File(compiled));
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

    /// The problems the language servers found in the files of the project that are not open.
    pub(super) fn server_problems(&self) -> Vec<Problem> {
        self.languages
            .workspace_diagnostics()
            .into_iter()
            .filter(|(path, _)| path.starts_with(&self.root))
            .filter(|(path, _)| !self.documents.iter().any(|d| d.path.as_ref() == Some(path)))
            .flat_map(|(path, list)| {
                list.iter()
                    .filter(|d| d.severity <= 3)
                    .map(|d| problem_of_server(&path, d))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// What the check before a commit finds in these files: for an open document what the editor
    /// shows, for the others their syntax, the marks of a merge and what the language servers say.
    pub(super) fn check_before_commit(&self, files: &[PathBuf]) -> Vec<Problem> {
        if self.settings.commit_check_off {
            return Vec::new();
        }
        let servers = self.languages.workspace_diagnostics();
        let known = crate::plugins::builtin::Host::problems(self);
        let mut found: Vec<Problem> = Vec::new();
        for file in files {
            let canonical = file.canonicalize().unwrap_or_else(|_| file.clone());
            let open = self
                .documents
                .iter()
                .find(|d| d.path.as_ref() == Some(&canonical));
            match open {
                Some(doc) => {
                    found.extend(known.iter().filter(|p| p.path == canonical).cloned());
                    found.extend(conflict_markers(&canonical, &doc.text));
                }
                None => {
                    found.extend(inspect_file(file));
                    for (path, list) in &servers {
                        if path == &canonical || path == file {
                            found.extend(
                                list.iter()
                                    .filter(|d| d.severity == 1)
                                    .map(|d| problem_of_server(path, d)),
                            );
                        }
                    }
                }
            }
        }
        found
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
