//! Refactorings that the IDE does by itself, on the text, without a language server: Extract
//! Variable (Ctrl+Alt+V), Extract Constant (Ctrl+Alt+C), Inline Variable (Ctrl+Alt+N) and, for
//! JavaScript and TypeScript, Extract Method (Ctrl+Alt+M). A language server adds its own to the
//! list of Ctrl+Alt+Shift+T (move, change the signature…).
//!
//! The functions at the top work on a text and answer with edits; the end of the file is what
//! the editor does with them.

use super::{EditorState, INDENT, is_word};
use crate::languages::TextEdit;
use std::ops::Range;

/// What a refactoring changes in the text, and where the new name is, to be renamed.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub edits: Vec<TextEdit>,
    /// The offset (after the edits) of the name the refactoring introduced.
    pub name_at: Option<usize>,
}

pub fn is_js(language: &str) -> bool {
    matches!(language, "js" | "ts" | "jsx" | "tsx")
}

/// The line holding `offset`: its start and its end, line break excluded.
fn line_of(text: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    start..end
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// How many more parentheses and square brackets the text opens than it closes, the ones in
/// strings and after `//` left out.
fn open_parens(text: &str) -> isize {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut previous = ' ';
    for c in text.chars() {
        if escaped {
            escaped = false;
        } else if let Some(open) = quote {
            if c == '\\' {
                escaped = true;
            } else if c == open {
                quote = None;
            }
        } else if c == '/' && previous == '/' {
            break;
        } else {
            match c {
                '"' | '`' => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        previous = c;
    }
    depth
}

/// The start of the statement that holds `offset`: the start of its first line, the lines of an
/// argument list that goes on from the line above being part of it.
pub fn statement_start(text: &str, offset: usize) -> usize {
    let mut starts: Vec<usize> = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let line = starts.partition_point(|&s| s <= offset) - 1;
    // The depth at the start of each line, up to the one of the offset.
    let mut depth = vec![0isize; line + 1];
    for l in 1..=line {
        let previous = &text[starts[l - 1]..starts[l] - 1];
        depth[l] = depth[l - 1] + open_parens(previous);
    }
    let mut l = line;
    while l > 0 && depth[l] > 0 {
        l -= 1;
    }
    starts[l]
}

/// A name for what an expression holds, from the expression.
pub fn suggest_name(expression: &str) -> String {
    let e = expression.trim();
    if e.starts_with(['"', '\'', '`']) {
        return "text".into();
    }
    if e.parse::<f64>().is_ok() {
        return "value".into();
    }
    if matches!(e, "true" | "false") {
        return "flag".into();
    }
    // `a.b.getCount(x)` is a count, `compute(x)` a result.
    let is_call = e.contains('(');
    let before_call = e.split('(').next().unwrap_or(e);
    let last = before_call
        .rsplit(['.', ':'])
        .next()
        .unwrap_or(before_call)
        .trim();
    let stripped = last
        .strip_prefix("get_")
        .or_else(|| last.strip_prefix("get"))
        .filter(|w| !w.is_empty());
    let word = match stripped {
        Some(w) => w,
        None if is_call => return "result".into(),
        None => last,
    };
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() && word.chars().all(|c| is_word(c) || c == '$') => {
            first.to_lowercase().chain(chars).collect()
        }
        _ => "extracted".into(),
    }
}

/// Whether `name` appears as a whole word in `text`.
fn uses_word(text: &str, name: &str) -> bool {
    words(text).iter().any(|(_, w)| w == name)
}

/// A name that nothing in `text` uses: `name`, else `name2`, `name3`…
pub fn free_name(text: &str, name: &str) -> String {
    if !uses_word(text, name) {
        return name.to_string();
    }
    (2..)
        .map(|n| format!("{name}{n}"))
        .find(|candidate| !uses_word(text, candidate))
        .expect("an endless range")
}

/// The words of a text (identifiers, keywords, numbers), with their ranges.
fn words(text: &str) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let part = is_word(c) || c == '$';
        match (start, part) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                out.push((s..i, text[s..i].to_string()));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s..text.len(), text[s..].to_string()));
    }
    out
}

/// The code with its strings and comments blanked out, the same length: to read the words in.
fn mask(code: &str) -> String {
    let mut masked = String::with_capacity(code.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            masked.extend(std::iter::repeat_n(' ', c.len_utf8()));
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == open {
                quote = None;
            }
        } else if c == '/' && chars.peek() == Some(&'/') {
            masked.push(' ');
            for rest in chars.by_ref() {
                if rest == '\n' {
                    masked.push('\n');
                    break;
                }
                masked.extend(std::iter::repeat_n(' ', rest.len_utf8()));
            }
        } else if matches!(c, '"' | '\'' | '`') {
            quote = Some(c);
            masked.push(' ');
        } else {
            masked.push(c);
        }
    }
    masked
}

/// The identifiers a piece of JavaScript uses, in order: not the ones in strings and comments, not
/// the properties after a dot, not the keywords.
fn identifiers(code: &str) -> Vec<(Range<usize>, String)> {
    const KEYWORDS: [&str; 44] = [
        "const", "let", "var", "function", "return", "if", "else", "for", "while", "do", "switch",
        "case", "break", "continue", "new", "delete", "typeof", "instanceof", "in", "of", "this",
        "null", "undefined", "true", "false", "async", "await", "try", "catch", "finally", "throw",
        "class", "extends", "super", "import", "export", "from", "default", "yield", "void", "static",
        "get", "set", "as",
    ];
    let masked = mask(code);
    words(&masked)
        .into_iter()
        .filter(|(range, word)| {
            let before = masked[..range.start].trim_end().chars().next_back();
            let property = before == Some('.');
            let key = masked[range.end..].trim_start().starts_with(':')
                && matches!(before, Some('{') | Some(','));
            !property
                && !key
                && !KEYWORDS.contains(&word.as_str())
                && !word.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .collect()
}

/// The names a JavaScript text declares: `const a`, `let b`, `var c`, and the parameters in the
/// parentheses of a function header.
fn declared(code: &str) -> Vec<String> {
    let ids = identifiers(code);
    let mut out = Vec::new();
    for (range, name) in &ids {
        let before: Vec<&str> = code[..range.start].split_whitespace().rev().take(1).collect();
        if matches!(before.first(), Some(&"const" | &"let" | &"var")) {
            out.push(name.clone());
        }
    }
    out
}

/// Whether the identifiers `ids` of `code` give a new value to `name`.
fn is_assigned(code: &str, ids: &[(Range<usize>, String)], name: &str) -> bool {
    ids.iter().filter(|(_, w)| w == name).any(|(range, _)| {
        let after = code[range.end..].trim_start();
        let before = code[..range.start].trim_end();
        (after.starts_with('=') && !after.starts_with("==") && !after.starts_with("=>"))
            || ["+=", "-=", "*=", "/=", "%=", "++", "--"]
                .iter()
                .any(|op| after.starts_with(op))
            || before.ends_with("++")
            || before.ends_with("--")
    })
}

fn function_header(line: &str) -> bool {
    let l = line.trim();
    if l.starts_with("if ") || l.starts_with("if(") || l.starts_with("for") || l.starts_with("while")
        || l.starts_with("switch") || l.starts_with("catch") || l.starts_with("} else")
        || l.starts_with("else") || l.starts_with("try")
    {
        return false;
    }
    l.contains("function") || l.contains("=>") || {
        // A method: `name(args) {`.
        let open = l.find('(');
        open.is_some_and(|i| {
            l[..i]
                .trim_start_matches("async ")
                .trim()
                .chars()
                .all(|c| is_word(c) || c == '$')
                && l.ends_with('{')
                && i > 0
        })
    }
}

/// The names of the parameters in the parentheses of a function header.
fn header_parameters(line: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (line.find('('), line.rfind(')')) else {
        return Vec::new();
    };
    if close < open {
        return Vec::new();
    }
    identifiers(&line[open + 1..close])
        .into_iter()
        .map(|(_, w)| w)
        .collect()
}

// ---- Extract variable and constant ---------------------------------------------------------

/// The declaration of a variable that holds an expression, in the language, without the
/// indentation and the line break.
fn declaration(language: &str, name: &str, expression: &str) -> String {
    match language {
        "rs" => format!("let {name} = {expression};"),
        l if is_js(l) => format!("const {name} = {expression};"),
        "java" => format!("var {name} = {expression};"),
        "kt" => format!("val {name} = {expression}"),
        "py" => format!("{name} = {expression}"),
        "go" => format!("{name} := {expression}"),
        "c" | "cpp" => format!("auto {name} = {expression};"),
        _ => format!("var {name} = {expression};"),
    }
}

/// The part of a selection that is the expression: trimmed, on one line, whole.
fn expression_of(text: &str, selection: &Range<usize>) -> Option<Range<usize>> {
    let selected = text.get(selection.clone())?;
    let trimmed = selected.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || open_parens(trimmed) != 0 {
        return None;
    }
    let start = selection.start + (selected.len() - selected.trim_start().len());
    Some(start..start + trimmed.len())
}

/// Extract Variable: the selected expression becomes a variable declared above the statement.
pub fn extract_variable(text: &str, selection: &Range<usize>, language: &str) -> Option<Change> {
    let range = expression_of(text, selection)?;
    let expression = &text[range.clone()];
    let at = statement_start(text, range.start);
    let indent = indent_of(&text[at..line_of(text, at).end]).to_string();
    let name = free_name(text, &suggest_name(expression));
    let inserted = format!("{indent}{}\n", declaration(language, &name, expression));
    let name_at = range.start + inserted.len();
    Some(Change {
        edits: vec![
            TextEdit {
                range: at..at,
                new_text: inserted,
            },
            TextEdit {
                range,
                new_text: name,
            },
        ],
        name_at: Some(name_at),
    })
}

/// A constant for an expression: where it goes (an offset) and its text, for the languages that
/// can say its type from the expression.
fn constant_declaration(language: &str, name: &str, expression: &str) -> Option<String> {
    let e = expression.trim();
    let is_string = e.starts_with(['"', '\'']) && e.len() >= 2;
    let is_int = e.parse::<i64>().is_ok();
    let is_float = e.parse::<f64>().is_ok() && !is_int;
    let is_bool = matches!(e, "true" | "false");
    match language {
        l if is_js(l) => Some(format!("const {name} = {e};")),
        "py" => Some(format!("{name} = {e}")),
        "rs" => {
            let kind = if is_string {
                "&str"
            } else if is_int {
                "i32"
            } else if is_float {
                "f64"
            } else if is_bool {
                "bool"
            } else {
                return None;
            };
            Some(format!("const {name}: {kind} = {e};"))
        }
        "java" => {
            let kind = if is_string {
                "String"
            } else if is_int {
                "int"
            } else if is_float {
                "double"
            } else if is_bool {
                "boolean"
            } else {
                return None;
            };
            Some(format!("private static final {kind} {name} = {e};"))
        }
        _ => None,
    }
}

/// Extract Constant: the selected expression becomes a constant at the top of the file (or of the
/// class, in Java).
pub fn extract_constant(
    text: &str,
    selection: &Range<usize>,
    language: &str,
    headers: &[usize],
    line_starts: &[usize],
) -> Option<Change> {
    let range = expression_of(text, selection)?;
    let expression = &text[range.clone()];
    let name = free_name(text, "CONSTANT");
    let declaration = constant_declaration(language, &name, expression)?;
    // Java: the first line of the body of the class; the others: above the top-level item.
    let (at, indent) = if language == "java" {
        let class = *headers.first()?;
        let after = line_starts.get(class + 1).copied().unwrap_or(text.len());
        (after, INDENT.to_string())
    } else {
        let top = headers
            .first()
            .and_then(|l| line_starts.get(*l))
            .copied()
            .unwrap_or_else(|| statement_start(text, range.start));
        (top, String::new())
    };
    let inserted = format!("{indent}{declaration}\n\n");
    let shift = if at <= range.start { inserted.len() } else { 0 };
    Some(Change {
        edits: vec![
            TextEdit {
                range: at..at,
                new_text: inserted,
            },
            TextEdit {
                range: range.clone(),
                new_text: name,
            },
        ],
        name_at: Some(range.start + shift),
    })
}

// ---- Inline variable -----------------------------------------------------------------------

/// A declaration `let name = expression;` on one line: the name, the initializer and the range of
/// the whole line with its line break.
fn declaration_at(text: &str, line: &Range<usize>, language: &str) -> Option<(String, String)> {
    let l = text[line.clone()].trim();
    let l = l.strip_suffix(';').unwrap_or(l).trim_end();
    let rest = if language == "py" {
        l
    } else {
        ["let mut ", "let ", "const ", "var ", "val ", "final var ", "final "]
            .iter()
            .find_map(|k| l.strip_prefix(k))?
    };
    let (name, value) = rest.split_once('=')?;
    if value.starts_with('=') || name.contains(['(', ',', '{', '[']) {
        return None;
    }
    // `let x: i32 = 1` and `int x = 1`.
    let name = name.split(':').next()?.trim();
    let name = name.rsplit(' ').next()?.trim();
    if name.is_empty() || !name.chars().all(|c| is_word(c) || c == '$') {
        return None;
    }
    let value = value.trim();
    (!value.is_empty()).then(|| (name.to_string(), value.to_string()))
}

/// Inline Variable: the uses of a variable (after its declaration, to the end of its block) are
/// replaced by what it holds, and the declaration goes. Why not, in the error.
pub fn inline_variable(
    text: &str,
    cursor: usize,
    language: &str,
    block_end: Option<usize>,
) -> Result<Change, String> {
    // The declaration is on the line of the caret, or it is the declaration of the name there.
    let here = line_of(text, cursor);
    let (name, value, declaration_line) = match declaration_at(text, &here, language) {
        Some((name, value)) => (name, value, here.clone()),
        None => {
            let word = words(text)
                .into_iter()
                .find(|(r, _)| r.start <= cursor && cursor <= r.end)
                .map(|(_, w)| w)
                .ok_or_else(|| tr!("inline_nothing").to_string())?;
            let found = text[..here.start]
                .match_indices('\n')
                .map(|(i, _)| i + 1)
                .chain(std::iter::once(0))
                .map(|s| line_of(text, s))
                .rev()
                .find(|l| declaration_at(text, l, language).is_some_and(|(n, _)| n == word))
                .ok_or_else(|| tr!("inline_no_declaration").to_string())?;
            let (name, value) = declaration_at(text, &found, language).expect("found above");
            (name, value, found)
        }
    };
    let after = (declaration_line.end + 1).min(text.len());
    let end = block_end.unwrap_or(text.len()).clamp(after, text.len());
    let scope = &text[after..end];
    let uses: Vec<Range<usize>> = words(scope)
        .into_iter()
        .filter(|(_, w)| *w == name)
        .map(|(r, _)| after + r.start..after + r.end)
        .collect();
    if uses.is_empty() {
        return Err(tr!("inline_no_uses").to_string());
    }
    // A variable that is assigned again cannot be replaced by its first value.
    for r in &uses {
        let next = text[r.end..].trim_start();
        let assigned = (next.starts_with('=') && !next.starts_with("=="))
            || ["+=", "-=", "*=", "/=", "++", "--"].iter().any(|op| next.starts_with(op));
        if assigned {
            return Err(tr!("inline_reassigned").to_string());
        }
    }
    let simple = value.chars().all(|c| is_word(c) || c == '$' || c == '.' || c == '"' || c == '\'')
        || (value.starts_with('(') && value.ends_with(')') && open_parens(&value) == 0);
    let replacement = if simple { value.clone() } else { format!("({value})") };
    let mut edits = vec![TextEdit {
        range: declaration_line.start..(declaration_line.end + 1).min(text.len()),
        new_text: String::new(),
    }];
    edits.extend(uses.into_iter().map(|range| TextEdit {
        range,
        new_text: replacement.clone(),
    }));
    Ok(Change {
        edits,
        name_at: None,
    })
}

// ---- Extract method (JavaScript and TypeScript) ----------------------------------------------

/// Extract Method: the selected lines become a function, called where they were. `headers` are
/// the first lines of the blocks that hold the selection, outermost first.
pub fn extract_method(
    text: &str,
    selection: &Range<usize>,
    headers: &[usize],
    line_starts: &[usize],
    block_end_of: &dyn Fn(usize) -> usize,
) -> Result<Change, String> {
    // The whole lines of the selection.
    let first = text[..selection.start].rfind('\n').map_or(0, |i| i + 1);
    let last_end = {
        let end = if selection.end > selection.start && text[..selection.end].ends_with('\n') {
            selection.end - 1
        } else {
            selection.end
        };
        text[end..].find('\n').map_or(text.len(), |i| end + i)
    };
    let body = &text[first..last_end];
    if body.trim().is_empty() {
        return Err(tr!("extract_method_select").to_string());
    }
    if open_parens(body) != 0 || body.matches('{').count() != body.matches('}').count() {
        return Err(tr!("extract_method_whole").to_string());
    }
    let ids = identifiers(body);
    let all_words = words(&mask(body));
    if all_words.iter().any(|(_, w)| w == "return") {
        return Err(tr!("extract_method_return").to_string());
    }
    // The function around: the innermost block header that is one, else the top of the file.
    let header_line = headers.iter().rev().copied().find(|l| {
        let start = line_starts[*l];
        function_header(&text[start..line_of(text, start).end])
    });
    let before_start = match header_line {
        Some(l) => line_starts[l],
        None => 0,
    };
    let before = &text[before_start..first];
    let mut known: Vec<String> = declared(before);
    if let Some(l) = header_line {
        let start = line_starts[l];
        known.extend(header_parameters(&text[start..line_of(text, start).end]));
    }
    let inside = declared(body);
    let mut params: Vec<String> = Vec::new();
    for (_, word) in &ids {
        if known.contains(word) && !inside.contains(word) && !params.contains(word) {
            params.push(word.clone());
        }
    }
    // What the code declares and the rest of the function uses.
    let rest_end = header_line
        .map(block_end_of)
        .unwrap_or(text.len())
        .clamp(last_end, text.len());
    let rest = &text[last_end..rest_end];
    // A parameter that the code gives a new value must come back: a number or a string is passed
    // by value.
    let assigned: Vec<&String> = params
        .iter()
        .filter(|p| is_assigned(body, &ids, p))
        .collect();
    let mut returned: Vec<&String> = inside.iter().filter(|n| uses_word(rest, n)).collect();
    for name in assigned.iter().copied() {
        if !returned.contains(&name) {
            returned.push(name);
        }
    }
    if returned.len() > 1 {
        return Err(tr!("extract_method_values").to_string());
    }
    let is_await = all_words.iter().any(|(_, w)| w == "await");
    let name = free_name(text, "extracted");
    let outer = match header_line {
        Some(l) => indent_of(&text[line_starts[l]..line_of(text, line_starts[l]).end]).to_string(),
        None => String::new(),
    };
    let selection_indent = indent_of(body).to_string();
    // The body, with the indentation of the selection replaced by the one of the function body.
    let inner = format!("{outer}{INDENT}");
    let mut function = String::new();
    function.push_str(&format!(
        "{outer}{}function {name}({}) {{\n",
        if is_await { "async " } else { "" },
        params.join(", ")
    ));
    for line in body.lines() {
        if line.trim().is_empty() {
            function.push('\n');
        } else {
            function.push_str(&inner);
            function.push_str(line.strip_prefix(selection_indent.as_str()).unwrap_or(line.trim_start()));
            function.push('\n');
        }
    }
    if let Some(value) = returned.first() {
        function.push_str(&format!("{inner}return {value};\n"));
    }
    function.push_str(&format!("{outer}}}\n\n"));
    let call = format!(
        "{}({})",
        name,
        params.join(", ")
    );
    let call = if is_await { format!("await {call}") } else { call };
    let statement = match returned.first() {
        Some(value) if assigned.contains(value) => format!("{selection_indent}{value} = {call};"),
        Some(value) => format!("{selection_indent}const {value} = {call};"),
        None => format!("{selection_indent}{call};"),
    };
    let insert_at = match header_line {
        Some(l) => line_starts[l],
        None => first,
    };
    let name_at = first + function.len() + selection_indent.len() + statement[selection_indent.len()..].find(&name).unwrap_or(0);
    Ok(Change {
        edits: vec![
            TextEdit {
                range: insert_at..insert_at,
                new_text: function,
            },
            TextEdit {
                range: first..last_end,
                new_text: statement,
            },
        ],
        name_at: Some(name_at).filter(|_| insert_at <= first),
    })
}

// ---- What the editor does with them ----------------------------------------------------------

/// A refactoring the IDE can do here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Native {
    ExtractVariable,
    ExtractConstant,
    ExtractMethod,
    InlineVariable,
}

impl Native {
    pub fn title(self) -> &'static str {
        match self {
            Native::ExtractVariable => tr!("extract_variable"),
            Native::ExtractConstant => tr!("extract_constant"),
            Native::ExtractMethod => tr!("extract_method"),
            Native::InlineVariable => tr!("inline_variable"),
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Native::ExtractVariable => "Ctrl+Alt+V",
            Native::ExtractConstant => "Ctrl+Alt+C",
            Native::ExtractMethod => "Ctrl+Alt+M",
            Native::InlineVariable => "Ctrl+Alt+N",
        }
    }
}

impl EditorState {
    fn line_starts(&self) -> Vec<usize> {
        self.lines.clone()
    }

    /// The first lines of the blocks (between braces) around the line of `offset`.
    fn headers_at(&self, text: &str, offset: usize) -> Vec<usize> {
        self.block_headers(text, self.line_of(offset))
    }

    /// The refactorings that fit the selection and the language.
    pub(super) fn native_refactorings(&self, text: &str, language: &str) -> Vec<Native> {
        let selection = self.selection();
        let mut found = Vec::new();
        if !selection.is_empty() && expression_of(text, &selection).is_some() {
            found.push(Native::ExtractVariable);
            let headers = self.headers_at(text, selection.start);
            if extract_constant(text, &selection, language, &headers, &self.line_starts()).is_some()
            {
                found.push(Native::ExtractConstant);
            }
        }
        if !selection.is_empty() && is_js(language) {
            found.push(Native::ExtractMethod);
        }
        if selection.is_empty()
            && (declaration_at(text, &line_of(text, self.cursor), language).is_some()
                || words(text)
                    .iter()
                    .any(|(r, _)| r.start <= self.cursor && self.cursor <= r.end))
        {
            found.push(Native::InlineVariable);
        }
        found
    }

    /// Does a refactoring of the IDE. The new name, when there is one, is selected for a rename.
    pub(super) fn run_native(&mut self, text: &mut String, language: &str, which: Native) {
        let selection = self.selection();
        let result: Result<Change, String> = match which {
            Native::ExtractVariable => extract_variable(text, &selection, language)
                .ok_or_else(|| tr!("extract_select_expression").to_string()),
            Native::ExtractConstant => {
                let headers = self.headers_at(text, selection.start);
                extract_constant(text, &selection, language, &headers, &self.line_starts())
                    .ok_or_else(|| tr!("extract_constant_unsupported").to_string())
            }
            Native::ExtractMethod => {
                let headers = self.headers_at(text, selection.start);
                let starts = self.line_starts();
                let ends: Vec<(usize, usize)> = self.regions.iter().map(|r| (r.first, r.last)).collect();
                let block_end_of = |line: usize| {
                    let last = ends
                        .iter()
                        .find(|(first, _)| *first == line)
                        .map_or(starts.len() - 1, |(_, last)| *last);
                    starts.get(last + 1).copied().unwrap_or(usize::MAX)
                };
                extract_method(text, &selection, &headers, &starts, &block_end_of)
            }
            Native::InlineVariable => {
                let line = self.line_of(self.cursor);
                // To the end of the block that holds the declaration.
                let headers = self.block_headers(text, line);
                let end = headers.last().and_then(|first| {
                    self.regions
                        .iter()
                        .find(|r| r.first == *first)
                        .map(|r| self.lines.get(r.last + 1).copied().unwrap_or(text.len()))
                });
                inline_variable(text, self.cursor, language, end)
            }
        };
        match result {
            Ok(change) => {
                let name_at = change.name_at;
                self.apply_edits(text, change.edits);
                if let Some(at) = name_at {
                    let word = self.word_at(text, at.min(text.len()));
                    self.select(word.start, word.end);
                    self.request_rename(text);
                }
            }
            Err(message) => self.assist.message = Some(message),
        }
    }
}

#[cfg(test)]
#[path = "../../test/editor/refactor_tests.rs"]
mod tests;
