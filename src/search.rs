//! Searching text: one document, or every file of the project. Plain text, whole words, case
//! sensitivity and regular expressions, and the replacement that goes with them.

use regex::{Regex, RegexBuilder};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Options {
    /// Upper and lower case are different.
    pub case: bool,
    /// Only whole words match.
    pub word: bool,
    /// The query is a regular expression.
    pub regex: bool,
}

/// The expression a query stands for. An error message when a regular expression is invalid.
pub fn build(query: &str, options: Options) -> Result<Regex, String> {
    let mut pattern = if options.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if options.word {
        pattern = format!(r"\b(?:{pattern})\b");
    }
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.case)
        .multi_line(true)
        .size_limit(10 * 1024 * 1024)
        .build()
        .map_err(|e| {
            let message = e.to_string();
            message
                .lines()
                .last()
                .unwrap_or(tr!("invalid_expression"))
                .trim()
                .to_string()
        })
}

/// The byte ranges of the matches, at most `limit`; matches of nothing are skipped.
pub fn find_all(text: &str, re: &Regex, limit: usize) -> Vec<Range<usize>> {
    re.find_iter(text)
        .filter(|m| !m.is_empty())
        .take(limit)
        .map(|m| m.range())
        .collect()
}

/// The text with every match replaced. With a regular expression `$1` stands for a group.
pub fn replace_all(text: &str, re: &Regex, replacement: &str, options: Options) -> String {
    if options.regex {
        re.replace_all(text, replacement).into_owned()
    } else {
        re.replace_all(text, regex::NoExpand(replacement))
            .into_owned()
    }
}

/// `*` and `?` against a name, case-insensitive.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    fn matches(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => matches(&p[1..], n) || (!n.is_empty() && matches(p, &n[1..])),
            (Some('?'), Some(_)) => matches(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a == b && matches(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    matches(&p, &n)
}

/// A comma-separated list of patterns (`*.rs, src/*`); empty matches everything. A pattern
/// with a slash is matched against the path from the project, the others against the file name.
pub fn mask_matches(mask: &str, relative: &Path) -> bool {
    let patterns: Vec<&str> = mask
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.is_empty() {
        return true;
    }
    let name = relative
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = relative.to_string_lossy().replace('\\', "/");
    patterns.iter().any(|p| {
        if p.contains('/') {
            glob_match(p.trim_start_matches("./"), &path)
                || glob_match(&format!("{}*", p.trim_end_matches('*')), &path)
        } else {
            glob_match(p, &name)
        }
    })
}

/// A match of a file, with its line for the list of results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineHit {
    /// 0-based line, and column and length in characters.
    pub line: usize,
    pub column: usize,
    pub length: usize,
    /// The line, without its indentation.
    pub text: String,
    /// Where the match starts in `text`, in characters.
    pub shown_column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHits {
    pub path: PathBuf,
    pub hits: Vec<LineHit>,
}

const MAX_FILE: u64 = 2 * 1024 * 1024;

/// The matches of one file's text, as lines.
pub fn hits_in(text: &str, re: &Regex, limit: usize) -> Vec<LineHit> {
    let mut hits = Vec::new();
    // Line starts, to find the line of a byte offset.
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    for range in find_all(text, re, limit) {
        let line = starts.partition_point(|&s| s <= range.start) - 1;
        let start = starts[line];
        let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        let full = text[start..end].trim_end_matches('\r');
        let indent = full.len() - full.trim_start().len();
        let shown = full.trim();
        let match_end = range.end.min(end);
        hits.push(LineHit {
            line,
            column: text[start..range.start].chars().count(),
            length: text[range.start..match_end].chars().count().max(1),
            shown_column: text[start + indent..range.start.max(start + indent)]
                .chars()
                .count(),
            text: shown.chars().take(300).collect(),
        });
    }
    hits
}

/// Looks for the matches in the files of the project, reporting each file that has some.
/// Stops when `cancel` is set or after `limit` matches. Returns the number of files looked at.
pub fn search_project(
    root: &Path,
    re: &Regex,
    mask: &str,
    cancel: &AtomicBool,
    limit: usize,
    mut report: impl FnMut(FileHits),
) -> usize {
    let mut stack = vec![root.to_path_buf()];
    let (mut looked, mut found) = (0, 0);
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(children) = crate::project::searchable_children(&dir) else {
            continue;
        };
        for child in children {
            if cancel.load(Ordering::Relaxed) || found >= limit {
                return looked;
            }
            if child.is_dir() {
                if !child.is_symlink() {
                    stack.push(child);
                }
                continue;
            }
            let relative = child.strip_prefix(root).unwrap_or(&child);
            if !mask_matches(mask, relative) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&child) else {
                continue;
            };
            if meta.len() > MAX_FILE {
                continue;
            }
            let Ok(bytes) = std::fs::read(&child) else {
                continue;
            };
            // Binary files: a zero byte early on.
            if bytes.iter().take(8000).any(|b| *b == 0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            looked += 1;
            let hits = hits_in(&text, re, limit - found);
            if !hits.is_empty() {
                found += hits.len();
                report(FileHits { path: child, hits });
            }
        }
    }
    looked
}

/// Replaces in every file of the project that has matches. Returns the files changed and
/// the replacements made.
pub fn replace_in_project(
    root: &Path,
    re: &Regex,
    replacement: &str,
    options: Options,
    mask: &str,
) -> (usize, usize) {
    let cancel = AtomicBool::new(false);
    let mut paths: Vec<PathBuf> = Vec::new();
    search_project(root, re, mask, &cancel, usize::MAX / 2, |hits| {
        paths.push(hits.path)
    });
    let (mut files, mut total) = (0, 0);
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let count = find_all(&text, re, usize::MAX).len();
        let replaced = replace_all(&text, re, replacement, options);
        if replaced != text && std::fs::write(&path, replaced).is_ok() {
            files += 1;
            total += count;
        }
    }
    (files, total)
}

#[cfg(test)]
#[path = "../test/search/tests.rs"]
mod tests;
