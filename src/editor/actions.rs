//! IntelliJ's editing actions that work on the text alone, whatever its language: Extend and
//! Shrink Selection (Ctrl+W, Ctrl+Shift+W), Join Lines (Ctrl+Shift+J), Toggle Case
//! (Ctrl+Shift+U), Move Line Up/Down (Alt+Shift+Up/Down) and Move Statement Up/Down
//! (Ctrl+Shift+Up/Down, a line, or a line with the block it opens).

use super::{EditorState, is_word};
use std::ops::Range;

/// The brackets Extend Selection steps out of.
const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
/// How far the brackets around the selection are looked for.
const LIMIT: usize = 200_000;

/// How many more brackets the line opens than it closes, the ones in quotes left out.
fn bracket_delta(line: &str) -> isize {
    let mut delta = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match (c, quote) {
            ('\\', Some(_)) => escaped = true,
            (q, Some(open)) if q == open => quote = None,
            (_, Some(_)) => {}
            ('"' | '`', None) => quote = Some(c),
            ('(' | '[' | '{', None) => delta += 1,
            (')' | ']' | '}', None) => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// The innermost bracket pair around `range`: the offsets of its two brackets.
fn enclosing_pair(text: &str, range: &Range<usize>) -> Option<(usize, usize)> {
    let mut depth: Vec<char> = Vec::new();
    let mut open = None;
    for (i, c) in text[..range.start].char_indices().rev().take(LIMIT) {
        if let Some((o, _)) = PAIRS.iter().find(|p| p.1 == c) {
            depth.push(*o);
        } else if PAIRS.iter().any(|p| p.0 == c) {
            if depth.last() == Some(&c) {
                depth.pop();
            } else if depth.is_empty() {
                open = Some((i, c));
                break;
            }
        }
    }
    let (at, opener) = open?;
    let closer = PAIRS.iter().find(|p| p.0 == opener)?.1;
    let mut level = 0usize;
    for (i, c) in text[at..].char_indices().skip(1).take(LIMIT) {
        if c == opener {
            level += 1;
        } else if c == closer {
            if level == 0 {
                let close = at + i;
                return (close >= range.end).then_some((at, close));
            }
            level -= 1;
        }
    }
    None
}

/// The quoted strings of the line `line` (its bounds in `text`) that hold `range`: their insides
/// and the strings with their quotes.
fn enclosing_quotes(text: &str, line: Range<usize>, range: &Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut open: Option<(usize, char)> = None;
    let mut escaped = false;
    for (i, c) in text[line.clone()].char_indices() {
        let at = line.start + i;
        if escaped {
            escaped = false;
            continue;
        }
        match (c, open) {
            ('\\', Some(_)) => escaped = true,
            ('"' | '\'' | '`', None) => open = Some((at, c)),
            (_, Some((start, quote))) if c == quote => {
                if start < range.start && at >= range.end {
                    out.push(start + 1..at);
                    out.push(start..at + 1);
                }
                open = None;
            }
            _ => {}
        }
    }
    out
}

impl EditorState {
    /// The ranges Extend Selection may go to from the selection, any size.
    fn expansions(&self, text: &str) -> Vec<Range<usize>> {
        let selection = self.selection();
        let mut out = Vec::new();
        // The word at the caret.
        let start = text[..selection.start]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(selection.start, |(i, _)| i);
        let end = text[selection.end..]
            .char_indices()
            .find(|(_, c)| !is_word(*c))
            .map_or(text.len(), |(i, _)| selection.end + i);
        out.push(start..end);
        // Strings, on the line of the selection.
        let first = self.line_of(selection.start);
        if first == self.line_of(selection.end) {
            out.extend(enclosing_quotes(
                text,
                self.line_range(text, first),
                &selection,
            ));
        }
        // Brackets, from the inside out: their insides, then with them.
        let mut around = selection.clone();
        for _ in 0..64 {
            let Some((open, close)) = enclosing_pair(text, &around) else {
                break;
            };
            let inside = open + 1..close;
            // The inside without the blank lines and the indentation around it.
            let trimmed_start = inside.start
                + (text[inside.clone()].len() - text[inside.clone()].trim_start().len());
            let trimmed_end = inside.start + text[inside.clone()].trim_end().len();
            if trimmed_start < trimmed_end {
                out.push(trimmed_start..trimmed_end);
            }
            out.push(inside);
            out.push(open..close + 1);
            around = open..close + 1;
        }
        // The lines, without their indentation, then whole.
        let lines = self.selected_lines(text);
        let block = &text[lines.clone()];
        let indent = block.len() - block.trim_start().len();
        out.push(lines.start + indent..lines.start + block.trim_end().len());
        out.push(lines.clone());
        out.push(0..text.len());
        out
    }

    /// Ctrl+W: the smallest range around the selection that is larger than it.
    pub(super) fn extend_selection(&mut self, text: &str) {
        let selection = self.selection();
        let next = self
            .expansions(text)
            .into_iter()
            .filter(|r| {
                r.start <= selection.start && r.end >= selection.end && r.len() > selection.len()
            })
            .min_by_key(|r| r.len());
        if self
            .expanded
            .last()
            .is_some_and(|(_, result)| *result != selection)
        {
            self.expanded.clear();
        }
        if let Some(next) = next {
            self.expanded
                .push(((self.anchor, self.cursor), next.clone()));
            self.anchor = next.start;
            self.cursor = next.end;
            self.scroll_to_cursor = true;
        }
    }

    /// Ctrl+Shift+W: back to the selection before the last Ctrl+W, while it is still the one.
    pub(super) fn shrink_selection(&mut self) {
        let selection = self.selection();
        match self.expanded.pop() {
            Some(((anchor, cursor), result)) if result == selection => {
                self.anchor = anchor;
                self.cursor = cursor;
            }
            _ => self.expanded.clear(),
        }
    }

    /// Ctrl+Shift+J: the next line, or the selected ones, joined to the first with one space.
    pub(super) fn join_lines(&mut self, text: &mut String) {
        let first = self.line_of(self.selection().start);
        let lines = self.selected_lines(text);
        let last = self.line_of(lines.end).max(first + 1);
        if last >= self.lines.len() {
            return;
        }
        let end = self.line_range(text, last).end;
        let mut joined = String::new();
        for (i, line) in text[self.lines[first]..end].split('\n').enumerate() {
            let line = line.trim_end_matches('\r');
            if i == 0 {
                joined.push_str(line.trim_end());
                continue;
            }
            let line = line.trim_start();
            if line.is_empty() {
                continue;
            }
            let glued = joined.ends_with(['(', '[']) || line.starts_with([')', ']', ',', ';', '.']);
            if !joined.is_empty() && !glued {
                joined.push(' ');
            }
            joined.push_str(line);
        }
        let start = self.lines[first];
        // The caret goes where the first line ended.
        let caret = start
            + text[start..self.line_range(text, first).end]
                .trim_end()
                .len();
        self.replace(text, start..end, &joined, false);
        self.anchor = caret;
        self.cursor = caret;
    }

    /// Ctrl+Shift+U: the selection, or the word at the caret, in upper case, or lower case when
    /// it has no lower case letter.
    pub(super) fn toggle_case(&mut self, text: &mut String) {
        let mut range = self.selection();
        if range.is_empty() {
            range = self.expansions(text).remove(0);
        }
        if range.is_empty() {
            return;
        }
        let old = &text[range.clone()];
        let new = if old.chars().any(char::is_lowercase) {
            old.to_uppercase()
        } else {
            old.to_lowercase()
        };
        let (anchor, cursor) = (self.anchor, self.cursor);
        let forward = cursor >= anchor;
        self.replace(text, range.clone(), &new, false);
        if anchor == cursor {
            self.anchor = anchor.min(range.start + new.len());
            self.cursor = self.anchor;
        } else if forward {
            self.anchor = range.start;
            self.cursor = range.start + new.len();
        } else {
            self.cursor = range.start;
            self.anchor = range.start + new.len();
        }
    }

    /// Alt+Shift+Up/Down: the selected lines swapped with the line above or below them.
    pub(super) fn move_lines(&mut self, text: &mut String, up: bool) {
        let block = self.selected_lines(text);
        let first = self.line_of(block.start);
        let last = self.line_of(block.end);
        let (range, new, shift) = if up {
            if first == 0 {
                return;
            }
            let above = self.line_range(text, first - 1);
            let new = format!("{}\n{}", &text[block.clone()], &text[above.clone()]);
            let shift = -((above.len() + 1) as isize);
            (above.start..block.end, new, shift)
        } else {
            if last + 1 >= self.lines.len() {
                return;
            }
            let below = self.line_range(text, last + 1);
            let new = format!("{}\n{}", &text[below.clone()], &text[block.clone()]);
            let shift = (below.len() + 1) as isize;
            (block.start..below.end, new, shift)
        };
        let (anchor, cursor) = (self.anchor, self.cursor);
        self.replace(text, range, &new, false);
        let moved = |at: usize| (at as isize + shift).max(0) as usize;
        self.anchor = moved(anchor);
        self.cursor = moved(cursor);
        self.scroll_to_cursor = true;
    }

    /// The first and last line of the statement that holds `line`: the line alone, or the line
    /// that opens a block with the block, or the line that closes one with the block.
    fn statement_at(&self, text: &str, line: usize) -> (usize, usize) {
        let delta = bracket_delta(self.line(text, line));
        if delta > 0 {
            let mut depth = delta;
            for next in line + 1..self.lines.len() {
                depth += bracket_delta(self.line(text, next));
                if depth <= 0 {
                    return (line, next);
                }
            }
            (line, self.lines.len() - 1)
        } else if delta < 0 {
            let mut depth = delta;
            for before in (0..line).rev() {
                depth += bracket_delta(self.line(text, before));
                if depth >= 0 {
                    return (before, line);
                }
            }
            (0, line)
        } else {
            (line, line)
        }
    }

    /// Ctrl+Shift+Up/Down: the statement at the caret (or the selected lines) swapped with the
    /// statement above or below it. A block goes whole; nothing leaves the block it is in.
    pub(super) fn move_statement(&mut self, text: &mut String, up: bool) {
        let (first, last) = if self.selection().is_empty() {
            self.statement_at(text, self.line_of(self.cursor))
        } else {
            let block = self.selected_lines(text);
            (self.line_of(block.start), self.line_of(block.end))
        };
        let here = self.lines[first]..self.line_range(text, last).end;
        let (other, down) = if up {
            if first == 0 || bracket_delta(self.line(text, first - 1)) > 0 {
                return;
            }
            let (a, b) = self.statement_at(text, first - 1);
            (self.lines[a]..self.line_range(text, b).end, false)
        } else {
            if last + 1 >= self.lines.len() || bracket_delta(self.line(text, last + 1)) < 0 {
                return;
            }
            let (a, b) = self.statement_at(text, last + 1);
            (self.lines[a]..self.line_range(text, b).end, true)
        };
        let (range, new, shift) = if down {
            let new = format!("{}\n{}", &text[other.clone()], &text[here.clone()]);
            (here.start..other.end, new, (other.len() + 1) as isize)
        } else {
            let new = format!("{}\n{}", &text[here.clone()], &text[other.clone()]);
            (other.start..here.end, new, -((other.len() + 1) as isize))
        };
        let (anchor, cursor) = (self.anchor, self.cursor);
        self.replace(text, range, &new, false);
        let moved = |at: usize| (at as isize + shift).max(0) as usize;
        self.anchor = moved(anchor);
        self.cursor = moved(cursor);
        self.scroll_to_cursor = true;
    }
}
