//! Private copies of the project where mutants run side by side.
//!
//! The original files are never touched. A worker directory is kept between runs so that build
//! caches stay warm; only the files that changed are copied again.

use super::discover::files_under;
use std::io;
use std::path::{Path, PathBuf};

const NOT_COPIED: &[&str] = &[".git", "target", ".metrics", "coverage", "node_modules"];

pub fn worker_dir(root: &Path, index: usize) -> PathBuf {
    root.join("target/mutation-workers")
        .join(format!("w{index}"))
}

fn differs(src: &Path, dst: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(src), std::fs::metadata(dst)) else {
        return true;
    };
    a.len() != b.len() || a.modified().ok() != b.modified().ok()
}

fn copy_changed(root: &Path, dst: &Path) -> io::Result<()> {
    for file in files_under(root, &|name| NOT_COPIED.contains(&name)) {
        let Ok(relative) = file.strip_prefix(root) else {
            continue;
        };
        let target = dst.join(relative);
        if differs(&file, &target) {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&file, &target)?;
        }
    }
    Ok(())
}

fn link_dir(original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let backslashes = |p: &Path| p.to_string_lossy().replace('/', "\\");
        let done = super::shell::hidden("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(backslashes(link))
            .arg(backslashes(original))
            .output()?;
        if done.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "mklink /J: {}",
                String::from_utf8_lossy(&done.stderr)
            )))
        }
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(original, link)
    }
}

/// Bring worker `index` up to date with the project: changed files copied, `node_modules` linked.
pub fn sync(root: &Path, index: usize) -> io::Result<PathBuf> {
    let dir = worker_dir(root, index);
    std::fs::create_dir_all(&dir)?;
    copy_changed(root, &dir)?;
    let modules = root.join("node_modules");
    if modules.is_dir() && !dir.join("node_modules").exists() {
        link_dir(&modules, &dir.join("node_modules"))?;
    }
    Ok(dir)
}

/// Delete every worker directory, unlinking `node_modules` first so the real one stays.
pub fn clean(root: &Path) {
    let base = root.join("target/mutation-workers");
    for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        let link = entry.path().join("node_modules");
        if link.exists() {
            let _ = std::fs::remove_dir(&link).or_else(|_| std::fs::remove_file(&link));
        }
    }
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/workers/tests.rs"]
mod tests;
