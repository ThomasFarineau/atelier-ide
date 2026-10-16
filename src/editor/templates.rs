//! Live templates and postfix templates in the completion, and Surround With (Ctrl+Alt+T).

use super::{
    EditorState, INDENT,
    assist::{Choice, Chooser, Entry},
    is_word,
    refactor::{free_name, suggest_name},
};
use crate::languages::{
    Completion, CompletionItem, ItemKind, QuickFix, TextEdit,
    templates::{self, Template},
};
use std::ops::Range;

/// The statements that surround code, for the languages with braces: the title, the line that
/// opens the block and the one that closes it.
fn blocks(language: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    match language {
        "rs" => vec![
            ("if", "if condition {", "}"),
            ("while", "while condition {", "}"),
            ("loop", "loop {", "}"),
            ("for", "for item in items {", "}"),
            ("unsafe", "unsafe {", "}"),
            ("{ }", "{", "}"),
        ],
        "js" | "ts" | "jsx" | "tsx" => vec![
            ("if", "if (condition) {", "}"),
            ("while", "while (condition) {", "}"),
            ("for", "for (const item of items) {", "}"),
            ("try / catch", "try {", "} catch (error) {\n}"),
            ("try / finally", "try {", "} finally {\n}"),
            ("{ }", "{", "}"),
        ],
        "java" | "c" | "cpp" | "cs" | "go" | "kt" => vec![
            ("if", "if (condition) {", "}"),
            ("while", "while (condition) {", "}"),
            ("for", "for (int i = 0; i < n; i++) {", "}"),
            ("try / catch", "try {", "} catch (Exception e) {\n}"),
            ("{ }", "{", "}"),
        ],
        // Python has no closing line: the body is indented under the header, and `try` is
        // followed by its `except`.
        "py" => vec![
            ("if", "if condition:", ""),
            ("while", "while condition:", ""),
            ("for", "for item in items:", ""),
            ("with", "with resource as value:", ""),
            ("try / except", "try:", "except Exception:\n    pass"),
        ],
        _ => Vec::new(),
    }
}

/// The marks that surround a part of a line: the title, what goes before, what goes after.
fn marks(language: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut marks = vec![
        ("( )", "(", ")"),
        ("[ ]", "[", "]"),
        ("\" \"", "\"", "\""),
        ("' '", "'", "'"),
    ];
    if matches!(language, "js" | "ts" | "jsx" | "tsx") {
        marks.push(("` `", "`", "`"));
        marks.push(("/* */", "/* ", " */"));
    } else if matches!(language, "rs" | "java" | "c" | "cpp" | "cs" | "go" | "kt") {
        marks.push(("/* */", "/* ", " */"));
    } else if matches!(language, "html" | "xml" | "md") {
        marks.push(("<div>", "<div>", "</div>"));
        marks.push(("<span>", "<span>", "</span>"));
        marks.push(("<!-- -->", "<!-- ", " -->"));
    }
    marks
}

/// Whether a language surrounds whole lines with a statement.
fn has_statements(language: &str) -> bool {
    !blocks(language).is_empty()
}

/// The whole lines holding `range`, line breaks left out.
fn whole_lines(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let end_from = if range.end > range.start && text[..range.end].ends_with('\n') {
        range.end - 1
    } else {
        range.end
    };
    let end = text[end_from..]
        .find('\n')
        .map_or(text.len(), |i| end_from + i);
    start..end
}

/// The surroundings offered for `selected` (a part of a line, or whole lines with `lines`): the
/// title and the text that replaces the range.
pub(super) fn surroundings(
    language: &str,
    text: &str,
    range: &Range<usize>,
    user: &[Template],
) -> (Range<usize>, Vec<(String, String)>) {
    let selected = &text[range.clone()];
    let multi_line = selected.contains('\n');
    let whole = if multi_line || selected.trim().is_empty() || has_statements(language) {
        whole_lines(text, range)
    } else {
        range.clone()
    };
    let mut found: Vec<(String, String)> = Vec::new();
    let body = &text[whole.clone()];
    let indent: String = body
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let on_lines = whole != *range || multi_line;
    if on_lines || selected.trim().is_empty() {
        for (title, open, close) in blocks(language) {
            let inner: Vec<String> = body
                .lines()
                .map(|l| {
                    if l.is_empty() {
                        String::new()
                    } else {
                        format!("{INDENT}{l}")
                    }
                })
                .collect();
            let mut new = format!("{indent}{open}\n{}", inner.join("\n"));
            if !close.is_empty() {
                new.push_str(&format!(
                    "\n{indent}{}",
                    close.replace('\n', &format!("\n{indent}"))
                ));
            }
            found.push((title.to_string(), new));
        }
    }
    if !(on_lines && multi_line) {
        let piece = if on_lines { body.trim() } else { selected };
        for (title, open, close) in marks(language) {
            let replaced = if on_lines {
                format!("{indent}{open}{piece}{close}")
            } else {
                format!("{open}{piece}{close}")
            };
            found.push((title.to_string(), replaced));
        }
    }
    for template in user.iter().filter(|t| t.surrounds() && t.is_for(language)) {
        let (new, _) = template.expand(body);
        let lines: Vec<&str> = new.lines().collect();
        let new = lines.join(&format!("\n{indent}"));
        let title = if template.description.is_empty() {
            template.abbreviation.clone()
        } else {
            template.description.clone()
        };
        found.push((title, format!("{indent}{new}")));
    }
    (whole, found)
}

/// The expression that ends just before `end` (the dot of a postfix template): names, dots,
/// calls, indexes and strings, not the operators, the commas or what is in unclosed brackets.
pub fn expression_before(text: &str, end: usize) -> Option<Range<usize>> {
    let mut i = end.min(text.len());
    while let Some(c) = text[..i].chars().next_back() {
        match c {
            ')' | ']' => {
                // To the bracket that opens it.
                let (open, close) = if c == ')' { ('(', ')') } else { ('[', ']') };
                let mut depth = 0;
                let mut found = None;
                for (at, d) in text[..i].char_indices().rev().take(4000) {
                    if d == close {
                        depth += 1;
                    } else if d == open {
                        depth -= 1;
                        if depth == 0 {
                            found = Some(at);
                            break;
                        }
                    }
                }
                i = found?;
            }
            '"' | '\'' | '`' => {
                // To the quote that opens the string, on the same line.
                let line_start = text[..i].rfind('\n').map_or(0, |n| n + 1);
                let open = text[line_start..i - 1].rfind(c).map(|n| line_start + n)?;
                i = open;
            }
            c if is_word(c) || c == '$' || c == '.' => i -= c.len_utf8(),
            ':' if text[..i].ends_with("::") => i -= 2,
            _ => break,
        }
    }
    // A leading dot (`.foo`) is not an expression.
    let start = i + text[i..end].len() - text[i..end].trim_start_matches('.').len();
    (start < end).then_some(start..end)
}

impl EditorState {
    /// Ctrl+Alt+T: the ways to surround the selection (or the line), as a list to choose from.
    pub(super) fn open_surround(&mut self, text: &str, language: &str) {
        let range = self.selection();
        let (whole, options) = surroundings(language, text, &range, &templates::user());
        let range = if range.is_empty() {
            whole.clone()
        } else {
            range
        };
        // The statements work on whole lines; the marks on the selection itself.
        let entries: Vec<Entry> = options
            .into_iter()
            .map(|(title, new_text)| {
                let target = if new_text.starts_with(char::is_whitespace) || new_text.contains('\n')
                {
                    whole.clone()
                } else {
                    range.clone()
                };
                Entry {
                    label: title.clone(),
                    detail: String::new(),
                    kind: None,
                    indent: 0,
                    choice: Choice::Fix(QuickFix {
                        title,
                        edits: vec![TextEdit {
                            range: target,
                            new_text,
                        }],
                    }),
                }
            })
            .collect();
        if entries.is_empty() {
            self.assist.message = Some(tr!("nothing_to_surround").into());
            return;
        }
        self.assist.popup = None;
        self.assist.chooser = Some(Chooser::new(tr!("surround_with"), entries, true));
    }

    /// Adds the live templates whose abbreviation starts with what is typed to the completion,
    /// and the postfix templates after an expression and a dot.
    pub(super) fn with_templates(
        &self,
        text: &str,
        completion: Option<Completion>,
        language: &str,
        explicit: bool,
    ) -> Option<Completion> {
        let user = templates::user();
        let cursor = self.cursor.min(text.len());
        let mut from = cursor;
        for (i, c) in text[..cursor].char_indices().rev() {
            if !is_word(c) {
                break;
            }
            from = i;
        }
        let query = &text[from..cursor];
        if query.is_empty() && !explicit {
            return completion;
        }
        // After a dot: the postfix templates of the user, then those of the IDE.
        let mut items: Vec<CompletionItem> = Vec::new();
        if text[..from].ends_with('.')
            && let Some(expression) = expression_before(text, from - 1)
        {
            let source = &text[expression.clone()];
            let name = free_name(text, &suggest_name(source));
            let own = user.iter().filter(|t| t.is_postfix() && t.is_for(language));
            let builtin = templates::postfix(language);
            let mut seen: Vec<String> = Vec::new();
            for template in own.chain(builtin.iter()) {
                // A real member of the same name wins over a template.
                let member = completion
                    .as_ref()
                    .is_some_and(|c| c.items.iter().any(|i| i.label == template.abbreviation));
                if member
                    || !template.abbreviation.starts_with(query)
                    || seen.contains(&template.abbreviation)
                {
                    continue;
                }
                seen.push(template.abbreviation.clone());
                let (insert, caret) = template.expand_postfix(source, &name);
                items.push(
                    CompletionItem::new(template.abbreviation.clone(), ItemKind::Snippet)
                        .detail(template.description.clone())
                        .insert(insert, caret)
                        .start(expression.start)
                        // After the members of the language.
                        .rank(60),
                );
            }
        } else {
            items.extend(
                user.iter()
                    .filter(|t| !t.surrounds() && !t.is_postfix())
                    .filter(|t| !t.abbreviation.is_empty() && t.is_for(language))
                    .filter(|t| t.abbreviation.starts_with(query))
                    .map(|t| {
                        let (insert, caret) = t.expand("");
                        CompletionItem::new(t.abbreviation.clone(), ItemKind::Snippet)
                            .detail(t.description.clone())
                            .insert(insert, caret)
                            .rank(5)
                    }),
            );
        }
        if items.is_empty() {
            return completion;
        }
        match completion {
            Some(mut c) if c.from == from => {
                c.items.extend(items);
                Some(c)
            }
            Some(c) => Some(c),
            None => Some(Completion { from, items }),
        }
    }
}

#[cfg(test)]
#[path = "../../test/editor/templates_tests.rs"]
mod tests;
