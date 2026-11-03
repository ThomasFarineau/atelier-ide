//! Reformat Code for HTML and Pug, done by the IDE.
//!
//! HTML: each line is indented by the depth of the elements open at its start; lines are not
//! joined nor split. The content of `<pre>`, `<textarea>` and comments is left as it is; the
//! code of `<script>` and `<style>` keeps its own indentation, moved under its tag, and is then
//! formatted by its language (see `Service::format`). Pug's indentation is its structure:
//! only the spaces at the ends of the lines go.

use super::{data, lexer, tree};
use std::ops::Range;

fn columns(indent: &str) -> usize {
    indent.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// Pug: the spaces at the ends of the lines removed.
pub fn reformat_pug(text: &str) -> String {
    let mut out: Vec<&str> = text
        .split('\n')
        .map(|l| l.trim_end_matches([' ', '\t']))
        .collect();
    // A `\r` at the end stays: the line breaks of the file are kept.
    for line in &mut out {
        if line.ends_with('\r') {
            *line = line.trim_end_matches('\r');
        }
    }
    let crlf = text.contains("\r\n");
    out.join(if crlf { "\r\n" } else { "\n" })
}

pub fn reformat(text: &str) -> String {
    let unit = "    ".to_string();
    let scan = lexer::scan(text);
    let tree = tree::build(&scan);
    // What keeps its own lines: comments, `<pre>`, `<textarea>`; and the code of `<script>` and
    // `<style>`, moved as a whole.
    let mut verbatim: Vec<Range<usize>> = scan
        .tokens
        .iter()
        .filter(|t| t.kind == lexer::Kind::Comment || t.kind == lexer::Kind::Declaration)
        .map(|t| t.range.clone())
        .collect();
    let mut blocks: Vec<Range<usize>> = Vec::new();
    for element in &tree.elements {
        let tag = &scan.tags[element.open];
        match tag.name.as_str() {
            // Up to its closing tag included: the spaces before it are in the text.
            "pre" | "textarea" => verbatim.push(tag.range.end..element.end + 1),
            "script" | "style" => blocks.push(tag.range.end..element.end),
            _ => {}
        }
    }
    let unit = markup_unit(text, &scan.tokens, &verbatim, &blocks).unwrap_or(unit);
    // +1 where the content of an element starts, -1 where it ends.
    let mut changes: Vec<(usize, i32)> = Vec::new();
    for element in &tree.elements {
        let tag = &scan.tags[element.open];
        if data::is_void(&tag.name) || tag.self_closing || !tag.complete {
            continue;
        }
        changes.push((tag.range.end, 1));
        changes.push((element.end, -1));
    }
    changes.sort_unstable();
    let depth_at = |at: usize| -> usize {
        let index = changes.partition_point(|(p, _)| *p <= at);
        changes[..index].iter().map(|(_, d)| d).sum::<i32>().max(0) as usize
    };
    let crlf = text.contains("\r\n");
    let lines: Vec<(usize, &str)> = {
        let mut start = 0;
        text.split('\n')
            .map(|l| {
                let line = (start, l.trim_end_matches('\r'));
                start += l.len() + 1;
                line
            })
            .collect()
    };
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    // The block being moved: its first line's old and new indentation in columns.
    let mut shift: Option<(Range<usize>, isize)> = None;
    for (start, line) in &lines {
        let content = line.trim_start();
        let first = start + (line.len() - content.len());
        if let Some(range) = verbatim.iter().find(|r| r.start < *start && *start < r.end) {
            // Inside a comment or a `<pre>`: as it is, but for a comment, moved with its
            // first line.
            let moved = shift
                .as_ref()
                .filter(|(r, _)| r == range)
                .map(|(_, delta)| *delta);
            out.push(match moved {
                Some(delta) => move_line(line, delta, &unit),
                None => line.to_string(),
            });
            continue;
        }
        if content.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        if let Some(block) = blocks.iter().find(|r| r.start <= *start && first < r.end) {
            // The code of a script or a style: its lines keep their indentation relative to
            // the least indented one, under the tag.
            let base = block_indent(text, block);
            let depth = depth_at(first);
            let old = columns(&line[..line.len() - content.len()]);
            let relative = old.saturating_sub(base);
            out.push(format!(
                "{}{}{}",
                unit.repeat(depth),
                " ".repeat(relative),
                content.trim_end()
            ));
            continue;
        }
        // A line that goes on inside a tag (its attributes over several lines).
        let in_tag = scan
            .tags
            .iter()
            .find(|t| t.range.start < first && first < t.range.end);
        let depth = match in_tag {
            Some(tag) => depth_at(tag.range.start) + 1,
            None => depth_at(first),
        };
        let indent = unit.repeat(depth);
        // A comment that starts the line moves with its following lines.
        if let Some(range) = verbatim.iter().find(|r| r.start == first) {
            let old = columns(&line[..line.len() - content.len()]) as isize;
            shift = Some((range.clone(), columns(&indent) as isize - old));
        }
        out.push(format!("{indent}{}", content.trim_end()));
    }
    out.join(if crlf { "\r\n" } else { "\n" })
}

/// The indentation step of the markup: the least indentation of the lines that start with a
/// tag, outside code, `<pre>` and comments. None when no such line is indented.
fn markup_unit(
    text: &str,
    tokens: &[lexer::Token],
    verbatim: &[Range<usize>],
    blocks: &[Range<usize>],
) -> Option<String> {
    let mut best: Option<&str> = None;
    let mut start = 0;
    for line in text.split('\n') {
        let line_start = start;
        start += line.len() + 1;
        let content = line.trim_start();
        let indent = &line[..line.len() - content.len()];
        let first = line_start + indent.len();
        let inside = |r: &Range<usize>| r.start < first && first < r.end;
        if indent.is_empty()
            || !content.starts_with('<')
            || verbatim.iter().any(inside)
            || blocks.iter().any(inside)
            || !tokens
                .binary_search_by_key(&first, |t| t.range.start)
                .is_ok_and(|i| tokens[i].kind == lexer::Kind::TagOpen)
        {
            continue;
        }
        if best.is_none_or(|b| indent.len() < b.len()) {
            best = Some(indent);
        }
    }
    best.map(|indent| {
        if indent.starts_with('\t') {
            "\t".to_string()
        } else {
            indent.to_string()
        }
    })
}

/// The indentation, in columns, of the least indented line of a block of code.
fn block_indent(text: &str, block: &Range<usize>) -> usize {
    let body = &text[block.clone()];
    body.split('\n')
        .skip_while(|l| l.trim().is_empty())
        .filter(|l| !l.trim().is_empty())
        .map(|l| columns(&l[..l.len() - l.trim_start().len()]))
        .min()
        .unwrap_or(0)
}

/// A line moved right (or left) by `delta` columns.
fn move_line(line: &str, delta: isize, unit: &str) -> String {
    let content = line.trim_start();
    let old = columns(&line[..line.len() - content.len()]) as isize;
    let new = (old + delta).max(0) as usize;
    let indent = if unit == "\t" {
        format!("{}{}", "\t".repeat(new / 4), " ".repeat(new % 4))
    } else {
        " ".repeat(new)
    };
    format!("{indent}{}", content.trim_end())
}
