//! Local History: a copy of a file each time it is saved (and before another program changes it),
//! kept in the data folder of Atelier, to go back to an earlier version after the editor is
//! closed. The copies of a file are in a folder of their own, one text file per version named
//! after the time it was made.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Versions kept for a file; the oldest go first.
const MAX_VERSIONS: usize = 60;
/// Files bigger than this are not kept.
const MAX_SIZE: usize = 5 * 1024 * 1024;

/// A saved version of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub path: PathBuf,
    /// Milliseconds since 1970.
    pub at: u64,
    pub size: u64,
}

/// A name for a path that stays the same from one run to the next.
fn name_of(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn folder(base: &Path, root: &Path, file: &Path) -> PathBuf {
    base.join("history").join(name_of(root)).join(name_of(file))
}

/// The versions of `file`, the latest first.
pub fn list(base: &Path, root: &Path, file: &Path) -> Vec<Version> {
    let Ok(entries) = fs::read_dir(folder(base, root, file)) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let at = path.file_stem()?.to_str()?.parse().ok()?;
            Some(Version {
                at,
                size: entry.metadata().ok()?.len(),
                path,
            })
        })
        .collect();
    versions.sort_by_key(|v| std::cmp::Reverse(v.at));
    versions
}

/// Keeps `text` as the latest version of `file`, unless it is the one already kept.
pub fn record(base: &Path, root: &Path, file: &Path, text: &str) {
    if text.len() > MAX_SIZE {
        return;
    }
    let versions = list(base, root, file);
    if versions
        .first()
        .and_then(|v| fs::read_to_string(&v.path).ok())
        .is_some_and(|last| last == text)
    {
        return;
    }
    let dir = folder(base, root, file);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    // Two versions in one millisecond would share a name.
    let at = versions.first().map_or(now, |v| now.max(v.at + 1));
    if fs::write(dir.join(format!("{at}.txt")), text).is_err() {
        return;
    }
    for old in versions.iter().skip(MAX_VERSIONS - 1) {
        let _ = fs::remove_file(&old.path);
    }
}

/// The lines added and removed to go from `old` to `new`.
pub fn changes(old: &str, new: &str) -> (usize, usize) {
    let diff = similar::TextDiff::from_lines(old, new);
    let (mut added, mut removed) = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (added, removed)
}

#[cfg(test)]
#[path = "../../test/app/history/tests.rs"]
mod tests;
