//! Live templates in the completion, and Surround With (Ctrl+Alt+T).

use super::{
    EditorState, INDENT, is_word,
    assist::{Choice, Chooser, Entry},
};
use crate::{
    languages::{
        Completion, CompletionItem, ItemKind, QuickFix, TextEdit,
        templates::{self, Template},
    },
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
    }
    marks
}

fn is_c_like(language: &str) -> bool {
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
    let end = text[end_from..].find('\n').map_or(text.len(), |i| end_from + i);
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
    let whole = if multi_line || selected.trim().is_empty() || is_c_like(language) {
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
                .map(|l| if l.is_empty() { String::new() } else { format!("{INDENT}{l}") })
                .collect();
            let close = close.replace('\n', &format!("\n{indent}"));
            found.push((
                title.to_string(),
                format!("{indent}{open}\n{}\n{indent}{close}", inner.join("\n")),
            ));
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

impl EditorState {
    /// Ctrl+Alt+T: the ways to surround the selection (or the line), as a list to choose from.
    pub(super) fn open_surround(&mut self, text: &str, language: &str) {
        let range = self.selection();
        let (whole, options) = surroundings(language, text, &range, templates::user());
        let range = if range.is_empty() { whole.clone() } else { range };
        // The statements work on whole lines; the marks on the selection itself.
        let entries: Vec<Entry> = options
            .into_iter()
            .map(|(title, new_text)| {
                let target = if new_text.starts_with(char::is_whitespace) || new_text.contains('\n') {
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

    /// Adds the templates whose abbreviation starts with what is typed to the completion.
    pub(super) fn with_templates(
        &self,
        text: &str,
        completion: Option<Completion>,
        language: &str,
        explicit: bool,
    ) -> Option<Completion> {
        let user = templates::user();
        if user.is_empty() {
            return completion;
        }
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
        let items: Vec<CompletionItem> = user
            .iter()
            .filter(|t| !t.surrounds() && !t.abbreviation.is_empty() && t.is_for(language))
            .filter(|t| t.abbreviation.starts_with(query))
            .map(|t| {
                let (insert, caret) = t.expand("");
                CompletionItem::new(t.abbreviation.clone(), ItemKind::Snippet)
                    .detail(t.description.clone())
                    .insert(insert, caret)
                    .rank(5)
            })
            .collect();
        if items.is_empty() {
            return completion;
        }
        match completion {
            Some(mut c) if c.from == from => {
                c.items.extend(items);
                Some(c)
            }
            // Members after a dot are not for a template.
            Some(c) => Some(c),
            None => Some(Completion { from, items }),
        }
    }
}

#[cfg(test)]
#[path = "../../test/editor/templates_tests.rs"]
mod tests;
