//! Links in the terminal, as IntelliJ makes them: addresses (`https://…`) open in the browser,
//! paths of the project with an optional line and column (`src/main.rs:12:5`, the places of
//! compiler errors and stack traces) open in the editor.

use regex::Regex;
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Url(String),
    /// A file, with its 1-based line and column when the text gives them.
    File {
        path: PathBuf,
        line: Option<usize>,
        column: Option<usize>,
    },
}

static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b(?:https?|file)://[^\s<>"'`]+"#).expect("regex"));
/// A path with an extension, then maybe `:line` or `:line:column` (or `(line,column)`).
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<path>(?:[A-Za-z]:[\\/])?[\w.\-~@+]*(?:[\\/][\w.\-~@+]+)*\.[A-Za-z0-9]+)(?::(\d+)(?::(\d+))?|\((\d+),(\d+)\))?")
        .expect("regex")
});

/// Characters an address does not end with, as text around it often puts them.
fn trim_url(url: &str) -> &str {
    let mut end = url.len();
    while let Some(c) = url[..end].chars().next_back() {
        let unbalanced = |open: char, close: char| {
            c == close && url[..end].matches(open).count() < url[..end].matches(close).count()
        };
        if matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"')
            || unbalanced('(', ')')
            || unbalanced('[', ']')
        {
            end -= c.len_utf8();
        } else {
            break;
        }
    }
    &url[..end]
}

/// The links of a line of the terminal, as character ranges. `exists` says whether a path
/// names a file: only those become links.
pub fn find(line: &str, root: &Path, exists: impl Fn(&Path) -> bool) -> Vec<(Range<usize>, Link)> {
    let chars = |byte: usize| line[..byte].chars().count();
    let mut out: Vec<(Range<usize>, Link)> = Vec::new();
    for m in URL.find_iter(line) {
        let url = trim_url(m.as_str());
        out.push((
            chars(m.start())..chars(m.start() + url.len()),
            Link::Url(url.to_string()),
        ));
    }
    for caps in PATH.captures_iter(line) {
        let whole = caps.get(0).expect("match");
        let range = chars(whole.start())..chars(whole.end());
        if out
            .iter()
            .any(|(r, _)| r.start < range.end && range.start < r.end)
        {
            continue;
        }
        let number = |i: usize| caps.get(i).and_then(|m| m.as_str().parse::<usize>().ok());
        // `:line:column`, or `(line,column)` as .NET and MSBuild write it.
        let (line_no, column) = match (number(2), number(4)) {
            (Some(l), _) => (Some(l), number(3)),
            (None, Some(l)) => (Some(l), number(5)),
            _ => (None, None),
        };
        let given = Path::new(&caps["path"]);
        let path = if given.is_absolute() {
            given.to_path_buf()
        } else {
            root.join(given)
        };
        if exists(&path) {
            out.push((
                range,
                Link::File {
                    path,
                    line: line_no,
                    column,
                },
            ));
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(line: &str) -> Vec<(Range<usize>, Link)> {
        find(line, Path::new("/p"), |p| {
            p.extension().is_some_and(|e| e == "rs" || e == "js")
        })
    }

    #[test]
    fn addresses_lose_the_punctuation_around_them() {
        let found = links("See https://docs.rs/egui (docs), or http://localhost:5173/.");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].1, Link::Url("https://docs.rs/egui".into()));
        assert_eq!(found[1].1, Link::Url("http://localhost:5173/".into()));
        assert_eq!(found[0].0, 4..24);
        let wiki = links("https://en.wikipedia.org/wiki/Rust_(language)");
        assert_eq!(
            wiki[0].1,
            Link::Url("https://en.wikipedia.org/wiki/Rust_(language)".into())
        );
    }

    #[test]
    fn paths_of_the_project_keep_their_line_and_column() {
        let found = links("  --> src/app.rs:3245:9");
        assert_eq!(
            found,
            [(
                6..23,
                Link::File {
                    path: Path::new("/p").join("src/app.rs"),
                    line: Some(3245),
                    column: Some(9)
                }
            )]
        );
        let stack = links("    at main (lib/server.js:10:5)");
        assert!(matches!(
            &stack[0].1,
            Link::File {
                line: Some(10),
                column: Some(5),
                ..
            }
        ));
        let csharp = links("main.rs(4,2): error");
        assert!(matches!(
            &csharp[0].1,
            Link::File {
                line: Some(4),
                column: Some(2),
                ..
            }
        ));
        assert!(
            links("version 1.2.3 of notes.txt").is_empty(),
            "only files that exist"
        );
    }

    #[test]
    fn a_windows_path_keeps_its_drive() {
        let found = find(r"error at C:\work\shop\main.rs:7", Path::new("/p"), |_| {
            true
        });
        assert_eq!(
            found[0].1,
            Link::File {
                path: PathBuf::from(r"C:\work\shop\main.rs"),
                line: Some(7),
                column: None
            }
        );
    }
}
