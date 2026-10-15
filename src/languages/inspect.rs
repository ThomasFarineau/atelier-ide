//! Inspecting files that are not open: the syntax errors of the files of a project, read without
//! a language server, for Inspect Code and for the check before a commit.

use super::{Diagnostic, javascript, rust};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Files bigger than this are not inspected: the check is a comfort, not worth a stall.
pub const MAX_SIZE: u64 = 1_500_000;

/// The language of a source file as `project::language` names it, when the inspection can read it.
pub fn language_of(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => "rs",
        "java" => "java",
        "js" | "jsx" | "mjs" | "cjs" => "js",
        "ts" | "tsx" | "mts" | "cts" => "ts",
        _ => return None,
    })
}

/// The syntax errors of a file.
pub fn syntax_errors(path: &Path, text: &str) -> Vec<Diagnostic> {
    match language_of(path) {
        Some("rs") => rust::analysis::syntax_errors(text),
        Some("java") => super::java::analysis::syntax_errors(text),
        Some(language) => {
            let _dialect = javascript::analysis::use_dialect(
                javascript::analysis::Dialect::of(Some(path), language),
            );
            javascript::analysis::syntax_errors(text)
        }
        None => Vec::new(),
    }
}

/// A problem of the compiler of TypeScript: where, how bad and what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    pub path: PathBuf,
    /// 0-based.
    pub line: usize,
    pub column: usize,
    pub error: bool,
    pub message: String,
}

/// The problems in the output of `tsc --pretty false`: lines such as
/// `src/a.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.`
pub fn parse_tsc(output: &str, root: &Path) -> Vec<Compiled> {
    output
        .lines()
        .filter_map(|line| {
            let (place, rest) = line.split_once("): ")?;
            let (file, position) = place.rsplit_once('(')?;
            let (row, column) = position.split_once(',')?;
            let (kind, message) = rest.split_once(' ')?;
            let error = match kind {
                "error" => true,
                "warning" => false,
                _ => return None,
            };
            Some(Compiled {
                path: root.join(file),
                line: row.trim().parse::<usize>().ok()?.saturating_sub(1),
                column: column.trim().parse::<usize>().ok()?.saturating_sub(1),
                error,
                message: message.trim().to_string(),
            })
        })
        .collect()
}

/// The compiler of TypeScript of the project, when it has one.
fn tsc_of(root: &Path) -> Option<PathBuf> {
    if !root.join("tsconfig.json").is_file() {
        return None;
    }
    let name = if cfg!(windows) { "tsc.cmd" } else { "tsc" };
    Some(root.join("node_modules").join(".bin").join(name)).filter(|p| p.is_file())
}

/// Runs `tsc --noEmit` on a project that has a `tsconfig.json` and its own compiler, for the type
/// errors the syntax does not show. Gives up after `limit`. Blocks: call it on a thread.
pub fn run_tsc(root: &Path, limit: Duration) -> Vec<Compiled> {
    let Some(tsc) = tsc_of(root) else {
        return Vec::new();
    };
    let mut command = Command::new(tsc);
    command
        .args(["--noEmit", "--pretty", "false"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = command.spawn() else {
        return Vec::new();
    };
    // The output is read on another thread so that a full pipe never stops the compiler.
    let Some(stdout) = child.stdout.take() else {
        return Vec::new();
    };
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut text = String::new();
        let _ = std::io::BufReader::new(stdout).read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                break;
            }
        }
    }
    parse_tsc(&reader.join().unwrap_or_default(), root)
}

#[cfg(test)]
#[path = "../../test/languages/inspect/tests.rs"]
mod tests;
