//! The intentions of the IDE, listed by Alt+Enter on the code under the caret: change the quotes
//! of a string, the operator of a comparison, `true` into `false`, `var` into `let`, put braces
//! around the statement of an `if` or take them off, make a Rust variable mutable, split a
//! string in two. They work on the line of the caret, whatever the language server knows.

use super::{EditorState, INDENT, is_word};
use crate::languages::{QuickFix, TextEdit};
use std::ops::Range;

fn is_js(language: &str) -> bool {
    matches!(language, "js" | "ts" | "jsx" | "tsx")
}

/// Languages whose blocks are in braces and may go without them for one statement.
fn braces_optional(language: &str) -> bool {
    matches!(language, "js" | "ts" | "jsx" | "tsx" | "java" | "c" | "cpp" | "cs" | "php")
}

/// The line of `offset`: where it starts and ends, line break left out.
fn line_range(text: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    start..end
}

/// The strings of a line that use these quotes, as (range with the quotes, quote).
fn strings(line: &str, quotes: &[char]) -> Vec<(Range<usize>, char)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, char)> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match open {
            Some((start, quote)) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == quote {
                    found.push((start..i + c.len_utf8(), quote));
                    open = None;
                }
            }
            None if quotes.contains(&c) => open = Some((i, c)),
            None => {}
        }
    }
    found
}

fn fix(title: String, range: Range<usize>, new_text: String) -> QuickFix {
    QuickFix {
        title,
        edits: vec![TextEdit { range, new_text }],
    }
}

/// `"abc"` becomes `'abc'` and the other way round, the quote inside escaped.
fn requote(literal: &str, to: char) -> String {
    let from = if to == '"' { '\'' } else { '"' };
    let inner = &literal[1..literal.len() - 1];
    let mut out = String::new();
    out.push(to);
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                // The old quote needs no escape any more.
                Some(next) if next == from => out.push(next),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            }
        } else if c == to {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out.push(to);
    out
}

/// The operators of a line, longest first: what each becomes when it can be swapped.
const TOKENS: [(&str, Option<&str>); 22] = [
    ("===", Some("!==")),
    ("!==", Some("===")),
    ("==", Some("!=")),
    ("!=", Some("==")),
    ("<=", Some(">")),
    (">=", Some("<")),
    ("&&", Some("||")),
    ("||", Some("&&")),
    ("->", None),
    ("=>", None),
    ("<<", None),
    (">>", None),
    ("++", None),
    ("--", None),
    ("+=", None),
    ("-=", None),
    ("*=", None),
    ("/=", None),
    ("<", Some(">=")),
    (">", Some("<=")),
    ("+", Some("-")),
    ("-", Some("+")),
];

/// The operators of a line outside the strings: (range in the line, operator, its swap).
fn operators(line: &str) -> Vec<(Range<usize>, &'static str, Option<&'static str>)> {
    let mut found = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut i = 0;
    while i < line.len() {
        let c = line[i..].chars().next().expect("inside the line");
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == open {
                quote = None;
            }
        } else if matches!(c, '"' | '\'' | '`') {
            quote = Some(c);
        } else if line[i..].starts_with("//") {
            break;
        } else if let Some((token, other)) = TOKENS.iter().find(|(t, _)| line[i..].starts_with(t)) {
            found.push((i..i + token.len(), *token, *other));
            i += token.len();
            continue;
        }
        i += c.len_utf8();
    }
    found
}

/// The intentions for the code at `cursor` (and the selection), as edits.
pub fn intentions(text: &str, cursor: usize, language: &str) -> Vec<QuickFix> {
    let line = line_range(text, cursor);
    let content = &text[line.clone()];
    let column = cursor.saturating_sub(line.start).min(content.len());
    let mut found: Vec<QuickFix> = Vec::new();
    let indent = &content[..content.len() - content.trim_start().len()];
    let trimmed = content.trim();

    // The quotes of a string.
    if is_js(language) || language == "py" {
        for (range, quote) in strings(content, &['"', '\'']) {
            if range.start <= column && column <= range.end {
                let to = if quote == '"' { '\'' } else { '"' };
                let title = if to == '\'' {
                    tr!("intention_single_quotes")
                } else {
                    tr!("intention_double_quotes")
                };
                found.push(fix(
                    title.to_string(),
                    line.start + range.start..line.start + range.end,
                    requote(&content[range.clone()], to),
                ));
            }
        }
    }

    // Split a string in two where the caret is.
    if !language.is_empty() && language != "rs" {
        for (range, quote) in strings(content, &['"', '\'']) {
            if range.start + 1 < column && column < range.end - 1 {
                let at = line.start + column;
                // Not in the middle of an escape.
                if text[..at].ends_with('\\') {
                    continue;
                }
                found.push(fix(
                    tr!("intention_split_string").to_string(),
                    at..at,
                    format!("{quote} + {quote}"),
                ));
            }
        }
    }

    // `true` and `false`.
    if let Some((range, word)) = word_at(content, column).filter(|(_, w)| w == "true" || w == "false") {
        let other = if word == "true" { "false" } else { "true" };
        found.push(fix(
            format!("{} {other}", tr!("intention_replace_with")),
            line.start + range.start..line.start + range.end,
            other.to_string(),
        ));
    }

    // The operator under the caret, or just before it.
    for (range, operator, other) in operators(content) {
        if !(range.start <= column && column <= range.end) {
            continue;
        }
        let Some(other) = other else { continue };
        // `+` and `-` only between operands: not a sign.
        if matches!(operator, "+" | "-") && !operand_around(content, range.start, operator.len()) {
            continue;
        }
        let replace = |from: &str, to: &str| {
            format!(
                "{} '{from}' {} '{to}'",
                tr!("intention_replace"),
                tr!("intention_with")
            )
        };
        found.push(fix(
            replace(operator, other),
            line.start + range.start..line.start + range.end,
            other.to_string(),
        ));
        // `==` into `===` in JavaScript.
        if is_js(language) && matches!(operator, "==" | "!=") {
            let strict = if operator == "==" { "===" } else { "!==" };
            found.push(fix(
                replace(operator, strict),
                line.start + range.start..line.start + range.end,
                strict.to_string(),
            ));
        }
        break;
    }

    // `var` into `let` or `const`.
    if is_js(language) && trimmed.starts_with("var ") {
        let at = line.start + indent.len();
        for keyword in ["let", "const"] {
            found.push(fix(
                format!("{} 'var' {} '{keyword}'", tr!("intention_replace"), tr!("intention_with")),
                at..at + 3,
                keyword.to_string(),
            ));
        }
    }

    // A Rust variable that can change, or cannot any more.
    if language == "rs" && trimmed.starts_with("let ") {
        let at = line.start + indent.len() + 4;
        if trimmed.starts_with("let mut ") {
            found.push(fix(tr!("intention_make_immutable").to_string(), at..at + 4, String::new()));
        } else {
            found.push(fix(tr!("intention_make_mutable").to_string(), at..at, "mut ".to_string()));
        }
    }

    // Braces round the statement of an `if`, `for`, `while` or `else`.
    if braces_optional(language) {
        if let Some(fixed) = add_braces(text, &line, language) {
            found.push(fixed);
        }
        if let Some(fixed) = remove_braces(text, &line) {
            found.push(fixed);
        }
    }
    found
}

/// The word at `column` of a line, with its range in the line.
fn word_at(line: &str, column: usize) -> Option<(Range<usize>, String)> {
    let mut start = column.min(line.len());
    while start > 0 && line[..start].chars().next_back().is_some_and(is_word) {
        start -= line[..start].chars().next_back()?.len_utf8();
    }
    let mut end = column.min(line.len());
    while end < line.len() && line[end..].chars().next().is_some_and(is_word) {
        end += line[end..].chars().next()?.len_utf8();
    }
    (start < end).then(|| (start..end, line[start..end].to_string()))
}

/// Whether the operator at `at` has an operand on each side.
fn operand_around(line: &str, at: usize, length: usize) -> bool {
    let before = line[..at].trim_end().chars().next_back();
    let after = line[at + length..].trim_start().chars().next();
    before.is_some_and(|c| is_word(c) || matches!(c, ')' | ']' | '"' | '\''))
        && after.is_some_and(|c| is_word(c) || matches!(c, '(' | '"' | '\'' | '['))
}

/// The end of the parentheses that open at `open` in `text`.
fn closing_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `if (a) b();` becomes a block of its own.
fn add_braces(text: &str, line: &Range<usize>, _language: &str) -> Option<QuickFix> {
    let content = &text[line.clone()];
    let indent = &content[..content.len() - content.trim_start().len()];
    let body = content.trim_start();
    // The part that opens the block and the statement after it.
    let keyword = ["else if", "if", "while", "for"]
        .into_iter()
        .find(|k| body.starts_with(k) && body[k.len()..].trim_start().starts_with('('));
    let (head, statement) = match keyword {
        Some(_) => {
            let open = body.find('(')?;
            let close = closing_paren(body, open)?;
            (&body[..=close], body[close + 1..].trim())
        }
        None => {
            let statement = body.strip_prefix("else ").map(|s| ("else", s));
            let statement = statement.or_else(|| body.strip_prefix("} else ").map(|s| ("} else", s)))?;
            (statement.0, statement.1.trim())
        }
    };
    if statement.is_empty()
        || statement.starts_with('{')
        || statement.starts_with("if")
        || statement.starts_with("//")
        || !statement.ends_with(';')
    {
        return None;
    }
    Some(fix(
        tr!("intention_add_braces").to_string(),
        line.clone(),
        format!("{indent}{head} {{\n{indent}{INDENT}{statement}\n{indent}}}"),
    ))
}

/// A block of one statement loses its braces.
fn remove_braces(text: &str, line: &Range<usize>) -> Option<QuickFix> {
    let content = &text[line.clone()];
    let indent = &content[..content.len() - content.trim_start().len()];
    let head = content.trim();
    let opens = head.ends_with('{')
        && ["if", "else", "for", "while", "} else"].iter().any(|k| head.starts_with(k));
    if !opens {
        return None;
    }
    let after = &text[(line.end + 1).min(text.len())..];
    let mut lines = after.lines();
    let statement = lines.next()?;
    let close = lines.next()?;
    if close.trim() != "}" || indent_of(close) != indent || statement.trim().is_empty() {
        return None;
    }
    // One statement: no block, no second statement on the line.
    let statement_text = statement.trim();
    if statement_text.ends_with('{') || statement_text.matches(';').count() > 1 {
        return None;
    }
    let head_without = head.trim_end_matches('{').trim_end();
    let end = line.end + 1 + statement.len() + 1 + close.len();
    Some(fix(
        tr!("intention_remove_braces").to_string(),
        line.start..end.min(text.len()),
        format!("{indent}{head_without} {statement_text}"),
    ))
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

impl EditorState {
    /// The intentions at the caret, for the list of Alt+Enter.
    pub(super) fn intention_fixes(&self, text: &str, language: &str) -> Vec<QuickFix> {
        intentions(text, self.cursor, language)
    }
}

#[cfg(test)]
#[path = "../../test/editor/intentions_tests.rs"]
mod tests;
