//! Reformat Code (Ctrl+Alt+L): what is common to the languages. A formatter gives the new text
//! of a document; the editor gets the smallest edits that lead to it, so that the caret, the
//! folds and the undo history stay on the code that did not move.

use super::TextEdit;
use similar::{DiffOp, TextDiff};

/// The edits that turn `old` into `new`, one per changed run of lines.
pub fn edits_between(old: &str, new: &str) -> Vec<TextEdit> {
    if old == new {
        return Vec::new();
    }
    let diff = TextDiff::from_lines(old, new);
    let old_lines: Vec<&str> = diff.old_slices().to_vec();
    let new_lines: Vec<&str> = diff.new_slices().to_vec();
    // Byte offset of each line of `old`, and of the end.
    let mut starts = Vec::with_capacity(old_lines.len() + 1);
    let mut at = 0;
    for line in &old_lines {
        starts.push(at);
        at += line.len();
    }
    starts.push(at);
    let mut edits = Vec::new();
    for op in diff.ops() {
        let (old_range, new_range) = match *op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index..old_index + old_len, new_index..new_index),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index..old_index + old_len,
                new_index..new_index + new_len,
            ),
        };
        edits.push(TextEdit {
            range: starts[old_range.start]..starts[old_range.end],
            new_text: new_lines[new_range].concat(),
        });
    }
    edits
}

/// The 0-based lines a byte range of `text` covers.
pub fn lines_of(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let line = |at: usize| text[..at.min(text.len())].matches('\n').count();
    let end = range.end.max(range.start);
    // A selection that ends at the start of a line does not take that line.
    let last = if end > range.start && text[..end.min(text.len())].ends_with('\n') {
        line(end) - 1
    } else {
        line(end)
    };
    line(range.start)..last + 1
}

/// The edits that fall in the given lines (0-based, end excluded): Reformat Code on the
/// selection, or on the lines changed since the last commit.
pub fn within_lines(
    edits: Vec<TextEdit>,
    text: &str,
    lines: &[std::ops::Range<usize>],
) -> Vec<TextEdit> {
    edits
        .into_iter()
        .filter(|e| {
            let touched = lines_of(text, e.range.clone());
            lines
                .iter()
                .any(|l| l.start <= touched.start && touched.end <= l.end.max(l.start + 1))
        })
        .collect()
}

/// How many lines the edits change, for the message after a reformat.
pub fn changed_lines(text: &str, edits: &[TextEdit]) -> usize {
    edits
        .iter()
        .map(|e| {
            let old = lines_of(text, e.range.clone()).len();
            old.max(e.new_text.lines().count()).max(1)
        })
        .sum()
}

/// JSON with the indentation the file already uses (two spaces when it has none), the order of
/// the keys kept.
pub fn json(text: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let indent = text
        .lines()
        .skip(1)
        .find_map(|line| {
            let spaces = line.len() - line.trim_start_matches(' ').len();
            if line.starts_with('\t') {
                Some("\t".to_string())
            } else {
                (spaces > 0).then(|| " ".repeat(spaces))
            }
        })
        .unwrap_or_else(|| "  ".to_string());
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(&value, &mut serializer).map_err(|e| e.to_string())?;
    let mut formatted = String::from_utf8(out).map_err(|e| e.to_string())?;
    if text.ends_with('\n') {
        formatted.push('\n');
    }
    Ok(formatted)
}

/// Applies edits to a text, as the editor does: for the tests and the formatters that need
/// the text after a first pass.
pub fn apply(text: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    let mut out = text.to_string();
    for edit in sorted {
        let end = edit.range.end.min(out.len());
        let start = edit.range.start.min(end);
        out.replace_range(start..end, &edit.new_text);
    }
    out
}

#[cfg(test)]
#[path = "../../test/languages/format/tests.rs"]
mod tests;
