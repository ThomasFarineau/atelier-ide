//! Bookmarks (F11 to set or remove one, Shift+F11 to list them): lines to come back to, kept for
//! the project in the data folder of Atelier. A bookmark remembers the text of its line, so that
//! it follows the line when code is added above it.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// How far from its line a bookmark looks for its text again.
const SEARCH: usize = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub path: PathBuf,
    /// 0-based.
    pub line: usize,
    /// The line, trimmed.
    pub text: String,
}

fn file_of(base: &Path, root: &Path) -> PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    base.join("bookmarks").join(format!("{hash:016x}.json"))
}

pub fn load(base: &Path, root: &Path) -> Vec<Bookmark> {
    fs::read_to_string(file_of(base, root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(base: &Path, root: &Path, bookmarks: &[Bookmark]) {
    let file = file_of(base, root);
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(bookmarks) {
        let _ = fs::write(file, text);
    }
}

/// Sets the bookmark of `line` of `path`, or removes it. Whether there is one now.
pub fn toggle(bookmarks: &mut Vec<Bookmark>, path: &Path, line: usize, text: &str) -> bool {
    if let Some(at) = bookmarks
        .iter()
        .position(|b| b.path == path && b.line == line)
    {
        bookmarks.remove(at);
        return false;
    }
    bookmarks.push(Bookmark {
        path: path.to_path_buf(),
        line,
        text: text.trim().to_string(),
    });
    true
}

/// Moves the bookmarks of `path` to the lines their text is on now: the nearest line that has it.
/// Whether any moved.
pub fn follow(bookmarks: &mut [Bookmark], path: &Path, lines: &[&str]) -> bool {
    let mut moved = false;
    for bookmark in bookmarks.iter_mut().filter(|b| b.path == path) {
        let here = lines.get(bookmark.line).map(|l| l.trim());
        if here == Some(bookmark.text.as_str()) || bookmark.text.is_empty() {
            continue;
        }
        let nearest = (1..=SEARCH).find_map(|d| {
            [bookmark.line.checked_sub(d), Some(bookmark.line + d)]
                .into_iter()
                .flatten()
                .find(|&l| lines.get(l).map(|t| t.trim()) == Some(bookmark.text.as_str()))
        });
        if let Some(line) = nearest {
            bookmark.line = line;
            moved = true;
        }
    }
    moved
}

#[cfg(test)]
#[path = "../../test/app/bookmarks/tests.rs"]
mod tests;
