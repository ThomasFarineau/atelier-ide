//! Find the source files a run should look at.

use super::lang::Lang;
use super::shell::hidden;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "test",
    "tests",
    "spec",
    "__tests__",
    "dist",
    "build",
    "coverage",
    "out",
    "venv",
];
const TEST_DIRS: &[&str] = &["test", "tests", "spec", "__tests__"];

/// A file in a known language that is not a test file.
pub fn is_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let test = name.contains(".test.") || name.contains(".spec.") || name.contains("_test.");
    Lang::from_path(path).is_some() && !test
}

fn skipped_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

/// The files under `dir`, without entering the folders `skip` names (`dir` itself is always
/// entered). Links are not followed.
pub fn files_under(dir: &Path, skip: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let (mut files, mut pending) = (Vec::new(), vec![dir.to_path_buf()]);
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && !skip(&entry.file_name().to_string_lossy()) {
                pending.push(entry.path());
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    files
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = files_under(dir, &skipped_dir);
    files.retain(|p| is_source(p));
    files
}

/// Every file in a known language, tests included, sorted: everywhere a function may be called from.
pub fn with_tests(root: &Path) -> Vec<PathBuf> {
    let skip = |name: &str| skipped_dir(name) && !TEST_DIRS.contains(&name);
    let mut files = files_under(root, &skip);
    files.retain(|p| Lang::from_path(p).is_some());
    files.sort();
    files
}

/// Path relative to `root`, with forward slashes.
pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn changed_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let out = hidden("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git status failed: {e}"))?;
    if !out.status.success() {
        return Err("git status failed (not a git repository?)".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|l| l.get(3..))
        .map(|l| root.join(l.rsplit(" -> ").next().unwrap_or("").trim_matches('"')))
        .filter(|p| p.is_file() && is_source(p) && !in_skipped_dir(root, p))
        .collect())
}

fn in_skipped_dir(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut comps: Vec<_> = rel.components().collect();
    comps.pop();
    comps
        .iter()
        .any(|c| skipped_dir(&c.as_os_str().to_string_lossy()))
}

fn resolve_arg(root: &Path, arg: &str) -> Vec<PathBuf> {
    let candidate = root.join(arg);
    if candidate.is_dir() {
        walk(&candidate)
    } else if candidate.is_file() {
        vec![candidate]
    } else {
        walk(root)
            .into_iter()
            .filter(|p| rel(root, p).contains(arg))
            .collect()
    }
}

/// Files named by `args` (existing paths, else path fragments), or the whole tree.
pub fn select(root: &Path, args: &[String], changed: bool) -> Result<Vec<PathBuf>, String> {
    let mut files = if changed {
        changed_files(root)?
    } else if args.is_empty() {
        walk(root)
    } else {
        args.iter().flat_map(|a| resolve_arg(root, a)).collect()
    };
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/discover/tests.rs"]
mod tests;
