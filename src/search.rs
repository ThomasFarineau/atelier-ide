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
    let mut pattern = if options.regex { query.to_string() } else { regex::escape(query) };
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
            message.lines().last().unwrap_or("expression invalide").trim().to_string()
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
        re.replace_all(text, regex::NoExpand(replacement)).into_owned()
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
    let patterns: Vec<&str> = mask.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    if patterns.is_empty() {
        return true;
    }
    let name = relative.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = relative.to_string_lossy().replace('\\', "/");
    patterns.iter().any(|p| {
        if p.contains('/') {
            glob_match(p.trim_start_matches("./"), &path) || glob_match(&format!("{}*", p.trim_end_matches('*')), &path)
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
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
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
            shown_column: text[start + indent..range.start.max(start + indent)].chars().count(),
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
        let Ok(children) = crate::project::searchable_children(&dir) else { continue };
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
            let Ok(meta) = std::fs::metadata(&child) else { continue };
            if meta.len() > MAX_FILE {
                continue;
            }
            let Ok(bytes) = std::fs::read(&child) else { continue };
            // Binary files: a zero byte early on.
            if bytes.iter().take(8000).any(|b| *b == 0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else { continue };
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
pub fn replace_in_project(root: &Path, re: &Regex, replacement: &str, options: Options, mask: &str) -> (usize, usize) {
    let cancel = AtomicBool::new(false);
    let mut paths: Vec<PathBuf> = Vec::new();
    search_project(root, re, mask, &cancel, usize::MAX / 2, |hits| paths.push(hits.path));
    let (mut files, mut total) = (0, 0);
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
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
mod tests {
    use super::*;

    #[test]
    fn options_decide_what_matches() {
        let text = "Cat cat concat CAT";
        let count = |query: &str, o: Options| find_all(text, &build(query, o).unwrap(), 100).len();
        assert_eq!(count("cat", Options::default()), 4, "case does not matter by default");
        assert_eq!(count("cat", Options { case: true, ..Default::default() }), 2);
        assert_eq!(count("cat", Options { word: true, ..Default::default() }), 3);
        assert_eq!(count("c.t", Options::default()), 0, "plain text: the dot is a dot");
        assert_eq!(count("c.t", Options { regex: true, ..Default::default() }), 4);
        assert!(build("(", Options { regex: true, ..Default::default() }).is_err());
        assert_eq!(find_all("aaa", &build("a*", Options { regex: true, ..Default::default() }).unwrap(), 10).len(), 1, "empty matches are skipped");
    }

    #[test]
    fn replacement_is_literal_unless_a_regex() {
        let plain = Options::default();
        let re = build("a.b", plain).unwrap();
        assert_eq!(replace_all("a.b axb", &re, "$1", plain), "$1 axb");
        let rx = Options { regex: true, ..Default::default() };
        let re = build(r"(\w+)@(\w+)", rx).unwrap();
        assert_eq!(replace_all("me@host", &re, "$2:$1", rx), "host:me");
    }

    #[test]
    fn masks_match_names_and_paths() {
        assert!(glob_match("*.rs", "Main.RS"));
        assert!(glob_match("a?c*", "abcdef"));
        assert!(!glob_match("*.rs", "main.js"));
        assert!(mask_matches("", Path::new("src/a.txt")));
        assert!(mask_matches("*.js, *.ts", Path::new("src/deep/a.ts")));
        assert!(!mask_matches("*.js", Path::new("src/a.ts")));
        assert!(mask_matches("src/*", Path::new("src/deep/a.ts")));
        assert!(!mask_matches("lib/*", Path::new("src/a.ts")));
    }

    #[test]
    fn hits_carry_their_line_and_column() {
        let text = "fn one() {}\n    let total = 1;\r\nlet total2 = total;\n";
        let re = build("total", Options { word: true, ..Default::default() }).unwrap();
        let hits = hits_in(text, &re, 10);
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].line, hits[0].column, hits[0].length), (1, 8, 5));
        assert_eq!(hits[0].text, "let total = 1;");
        assert_eq!(hits[0].shown_column, 4);
        assert_eq!((hits[1].line, hits[1].column), (2, 13));
    }

    #[test]
    fn the_project_is_searched_without_its_libraries() {
        let root = std::env::temp_dir().join(format!("atelier-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules").join("dep")).unwrap();
        std::fs::write(root.join("src").join("a.js"), "const needle = 1;\nneedle();\n").unwrap();
        std::fs::write(root.join("src").join("b.txt"), "a needle here\n").unwrap();
        std::fs::write(root.join("src").join("c.bin"), b"needle\0binary").unwrap();
        std::fs::write(root.join("node_modules").join("dep").join("x.js"), "needle\n").unwrap();
        let re = build("needle", Options::default()).unwrap();
        let cancel = AtomicBool::new(false);
        let mut found: Vec<(String, usize)> = Vec::new();
        let looked = search_project(&root, &re, "", &cancel, 100, |h| {
            found.push((h.path.file_name().unwrap().to_string_lossy().into_owned(), h.hits.len()));
        });
        found.sort();
        assert_eq!(found, [("a.js".to_string(), 2), ("b.txt".to_string(), 1)]);
        assert_eq!(looked, 2);
        let mut only_js = Vec::new();
        search_project(&root, &re, "*.js", &cancel, 100, |h| only_js.push(h.path));
        assert_eq!(only_js.len(), 1);
        // Replacing touches the same files.
        let (files, total) = replace_in_project(&root, &re, "pin", Options::default(), "*.js");
        assert_eq!((files, total), (1, 2));
        assert_eq!(std::fs::read_to_string(root.join("src").join("a.js")).unwrap(), "const pin = 1;\npin();\n");
        assert!(std::fs::read_to_string(root.join("src").join("b.txt")).unwrap().contains("needle"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
