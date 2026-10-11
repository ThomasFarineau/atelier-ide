//! Code folding: the regions of a document that can be collapsed to a few characters, found
//! from its brackets, block comments and runs of imports, whatever the language.

use std::ops::Range;

/// A collapsible region, spanning several lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// Offset of the token that opens it: the identity of the region while the text changes.
    pub open: usize,
    /// First and last line (0-based).
    pub first: usize,
    pub last: usize,
    /// Collapsed, the first line is shown up to here...
    pub head_end: usize,
    /// ... then a placeholder, then the last line from here.
    pub tail_start: usize,
}

/// Texts longer than this are not scanned: folding is a comfort, not worth a stall.
const LIMIT: usize = 4_000_000;

fn rank(open: u8) -> u8 {
    match open {
        b'{' => 0,
        b'[' => 1,
        b'(' => 2,
        _ => 3,
    }
}

fn line_of(lines: &[usize], pos: usize) -> usize {
    lines
        .partition_point(|&start| start <= pos)
        .saturating_sub(1)
}

/// The regions of `text`, sorted by first line, one per line (the braces win over the
/// brackets and parentheses that open on the same line).
pub fn regions(text: &str, lines: &[usize], language: &str) -> Vec<Region> {
    if text.len() > LIMIT {
        return Vec::new();
    }
    let c_like = matches!(
        language,
        "rs" | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "java"
            | "c"
            | "cpp"
            | "h"
            | "cs"
            | "go"
            | "php"
            | "css"
            | "kt"
            | "swift"
    );
    let line_comment: Option<&str> = match language {
        _ if c_like => Some("//"),
        "py" | "sh" | "yaml" | "yml" | "toml" | "rb" => Some("#"),
        "sql" => Some("--"),
        _ => None,
    };
    let rust = language == "rs";
    let templates = matches!(language, "js" | "jsx" | "ts" | "tsx" | "go");
    let bytes = text.as_bytes();
    let mut found: Vec<(u8, Region)> = Vec::new();
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(prefix) = line_comment
            && bytes[i..].starts_with(prefix.as_bytes())
        {
            i += text[i..].find('\n').unwrap_or(text.len() - i);
            continue;
        }
        match b {
            b'/' if c_like && bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..]
                    .find("*/")
                    .map_or(text.len(), |e| i + 2 + e + 2);
                let (first, last) = (line_of(lines, i), line_of(lines, end.saturating_sub(1)));
                if last > first && end >= i + 4 {
                    found.push((
                        4,
                        Region {
                            open: i,
                            first,
                            last,
                            head_end: i + 2,
                            tail_start: end - 2,
                        },
                    ));
                }
                i = end;
                continue;
            }
            b'"' | b'`' | b'\'' => {
                let quote = b;
                let multiline = quote == b'`' && templates;
                if quote == b'\'' {
                    if language_has_no_quotes(line_comment, c_like) {
                        i += 1;
                        continue;
                    }
                    if rust {
                        // A character literal, or a lifetime that has no closing quote.
                        let rest = &text[i + 1..];
                        let mut chars = rest.chars();
                        let literal = match chars.next() {
                            Some('\\') => {
                                rest.find('\'').filter(|&e| e > 0 && e <= 12).map(|e| e + 2)
                            }
                            Some(c) if c != '\'' => {
                                (chars.next() == Some('\'')).then(|| c.len_utf8() + 2)
                            }
                            _ => None,
                        };
                        i += literal.unwrap_or(1);
                        continue;
                    }
                }
                let mut j = i + 1;
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' => j += 2,
                        b'\n' if !multiline => break,
                        c if c == quote => {
                            j += 1;
                            break;
                        }
                        _ => j += 1,
                    }
                }
                i = j.min(bytes.len());
                continue;
            }
            b'{' | b'[' | b'(' => stack.push((b, i)),
            b'}' | b']' | b')' => {
                let open = match b {
                    b'}' => b'{',
                    b']' => b'[',
                    _ => b'(',
                };
                // Unclosed brackets above the match are given up on.
                if let Some(at) = stack.iter().rposition(|(kind, _)| *kind == open) {
                    let (_, start) = stack[at];
                    stack.truncate(at);
                    let (first, last) = (line_of(lines, start), line_of(lines, i));
                    if last > first {
                        found.push((
                            rank(open),
                            Region {
                                open: start,
                                first,
                                last,
                                head_end: start + 1,
                                tail_start: i,
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    imports(text, lines, &mut found);
    // One region per first line: the strongest, then the one that closes last.
    found.sort_by(|a, b| {
        a.1.first
            .cmp(&b.1.first)
            .then(a.0.cmp(&b.0))
            .then(b.1.last.cmp(&a.1.last))
    });
    found.dedup_by_key(|(_, r)| r.first);
    found.into_iter().map(|(_, r)| r).collect()
}

fn language_has_no_quotes(line_comment: Option<&str>, c_like: bool) -> bool {
    // Plain text, markdown, html…: an apostrophe is a word, not a string.
    line_comment.is_none() && !c_like
}

/// Runs of two or more `import`/`use` lines at the start of the line.
fn imports(text: &str, lines: &[usize], out: &mut Vec<(u8, Region)>) {
    const KEYWORDS: [&str; 3] = ["import ", "use ", "#include "];
    let keyword_of = |line: usize| -> Option<&'static str> {
        let start = lines[line];
        let rest = &text[start..];
        KEYWORDS.iter().copied().find(|k| rest.starts_with(k))
    };
    let mut line = 0;
    while line < lines.len() {
        let Some(keyword) = keyword_of(line) else {
            line += 1;
            continue;
        };
        let mut end = line;
        while end + 1 < lines.len() && keyword_of(end + 1) == Some(keyword) {
            end += 1;
        }
        if end > line {
            let last_end = lines.get(end + 1).map_or(text.len(), |&n| n - 1);
            let last_end = if text[..last_end].ends_with('\r') {
                last_end - 1
            } else {
                last_end
            };
            out.push((
                5,
                Region {
                    open: lines[line],
                    first: line,
                    last: end,
                    head_end: lines[line] + keyword.len(),
                    tail_start: last_end,
                },
            ));
        }
        line = end + 1;
    }
}

/// The lines hidden by the collapsed regions, as sorted, merged ranges.
pub fn hidden(regions: &[Region], collapsed: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = regions
        .iter()
        .filter(|r| collapsed.contains(&r.open))
        .map(|r| r.first + 1..r.last + 1)
        .collect();
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

#[cfg(test)]
#[path = "../../test/editor/fold/tests.rs"]
mod tests;
