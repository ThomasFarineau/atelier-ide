//! What the IDE knows of a style sheet by itself: syntax errors, the structure, where a
//! variable, a mixin or a class is declared and used, completion and Reformat Code. The
//! language server knows better and takes over once it runs; this answers before, or without
//! it, and for Sass, which the server does not read.

use super::{
    data,
    lexer::{self, Dialect, Kind, Role, Statement, Token},
};
use crate::languages::{
    Completion, CompletionItem, Diagnostic, ItemKind, Location, Severity, Symbol,
};
use std::ops::Range;

/// Bigger files are left to the server.
pub const MAX_TEXT: usize = 1_000_000;
const MAX_ERRORS: usize = 50;

// ---- Syntax errors -----------------------------------------------------------------------

pub fn syntax_errors(text: &str, dialect: Dialect) -> Vec<Diagnostic> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let (tokens, unclosed) = lexer::tokens(text, dialect);
    let error = |range: Range<usize>, message: String| Diagnostic {
        range,
        severity: Severity::Error,
        message,
        source: tr!("css.source").into(),
    };
    let mut out: Vec<Diagnostic> = unclosed
        .into_iter()
        .map(|u| {
            let message = if u.comment {
                tr!("css.unclosed_comment")
            } else {
                tr!("css.unclosed_string")
            };
            error(u.range, message.into())
        })
        .collect();
    // Braces, parentheses and brackets: each one closed by its own.
    let mut open: Vec<(u8, Range<usize>)> = Vec::new();
    for token in &tokens {
        let Kind::Punct(b) = token.kind else {
            if token.kind == Kind::Interpolation {
                let opening = text[token.range.clone()].starts_with('#');
                if opening {
                    open.push((b'#', token.range.clone()));
                } else if open.last().is_some_and(|(o, _)| *o == b'#') {
                    open.pop();
                }
            }
            continue;
        };
        let opener = match b {
            b'{' | b'(' | b'[' => {
                open.push((b, token.range.clone()));
                continue;
            }
            b'}' => b'{',
            b')' => b'(',
            b']' => b'[',
            _ => continue,
        };
        match open.last() {
            Some((o, _)) if *o == opener => {
                open.pop();
            }
            // `(` left open inside a block that closes: both are reported, once.
            Some((o, range)) if b == b'}' && *o != b'{' && open.iter().any(|(o, _)| *o == b'{') => {
                out.push(error(
                    range.clone(),
                    trf!("css.not_closed", what = *o as char),
                ));
                while open.last().is_some_and(|(o, _)| *o != b'{') {
                    open.pop();
                }
                open.pop();
            }
            _ => out.push(error(
                token.range.clone(),
                trf!("css.unexpected", what = b as char),
            )),
        }
        if out.len() >= MAX_ERRORS {
            return out;
        }
    }
    for (o, range) in open {
        let what = if o == b'#' { '{' } else { o as char };
        out.push(error(range, trf!("css.not_closed", what = what)));
    }
    // A declaration without a value: `color:;`.
    let statements = lexer::statements(text, &tokens, dialect);
    for stmt in &statements {
        if stmt.kind != Statement::Declaration || stmt.opens {
            continue;
        }
        let parts: Vec<&Token> = tokens[stmt.tokens.clone()]
            .iter()
            .filter(|t| !matches!(t.kind, Kind::Comment | Kind::Newline))
            .collect();
        if let [name, colon] = parts.as_slice()
            && colon.kind == Kind::Punct(b':')
        {
            out.push(Diagnostic {
                range: name.range.start..colon.range.end,
                severity: Severity::Warning,
                message: tr!("css.empty_value").into(),
                source: tr!("css.source").into(),
            });
        }
    }
    out.truncate(MAX_ERRORS);
    out
}

// ---- Names ---------------------------------------------------------------------------------

/// What a name is, for its declaration and its uses.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Name {
    /// `$x`, `--x`, `@x` (Less).
    Variable(String),
    Mixin(String),
    Function(String),
    /// `.x`, `%x`, `#x` in a selector or after `@extend`.
    Selector(String),
    Keyframes(String),
}

/// A name of the sheet: where it is, what it is, and whether it is declared there.
#[derive(Clone, Debug)]
struct Occurrence {
    name: Name,
    range: Range<usize>,
    declaration: bool,
}

fn occurrences(text: &str, dialect: Dialect) -> Vec<Occurrence> {
    let (tokens, _) = lexer::tokens(text, dialect);
    let roles = lexer::roles(text, &tokens, dialect);
    let statements = lexer::statements(text, &tokens, dialect);
    let mut out = Vec::new();
    for stmt in &statements {
        let range = stmt.tokens.clone();
        let mut meaningful = range
            .clone()
            .filter(|&i| !matches!(tokens[i].kind, Kind::Comment | Kind::Newline));
        let first = meaningful.next();
        let second = meaningful.next();
        let head = first.map(|i| &text[tokens[i].range.clone()]);
        let declares_variable = stmt.kind == Statement::Declaration
            && second.is_some_and(|i| tokens[i].kind == Kind::Punct(b':'));
        for i in range {
            let token = &tokens[i];
            let word = &text[token.range.clone()];
            let name = match (token.kind, roles[i]) {
                (Kind::Variable, _) => Some(Name::Variable(word.to_string())),
                (Kind::AtRule, Role::Variable) => Some(Name::Variable(word.to_string())),
                (Kind::Ident | Kind::Function, Role::Function) => {
                    let mixin = matches!(head, Some("@mixin" | "@include" | "=" | "+"));
                    if mixin {
                        Some(Name::Mixin(word.to_string()))
                    } else if head == Some("@function") {
                        Some(Name::Function(word.to_string()))
                    } else if head.is_some_and(|h| h.ends_with("keyframes")) {
                        Some(Name::Keyframes(word.to_string()))
                    } else {
                        Some(Name::Function(word.to_string()))
                    }
                }
                (Kind::Ident, Role::Class) => Some(Name::Selector(format!(".{word}"))),
                (Kind::Hash | Kind::Placeholder, Role::Class) => {
                    Some(Name::Selector(word.to_string()))
                }
                // `animation-name: spin`: a use of keyframes.
                (Kind::Ident, Role::Text) if stmt.kind == Statement::Declaration => {
                    let property = head.unwrap_or("");
                    (property == "animation" || property == "animation-name")
                        .then(|| Name::Keyframes(word.to_string()))
                }
                _ => None,
            };
            let Some(name) = name else { continue };
            let declaration = match &name {
                Name::Variable(_) => declares_variable && Some(i) == first,
                Name::Mixin(_) => matches!(head, Some("@mixin" | "=")),
                Name::Function(_) => head == Some("@function"),
                Name::Keyframes(_) => head.is_some_and(|h| h.ends_with("keyframes")),
                Name::Selector(_) => stmt.kind == Statement::Selector,
            };
            let range = if matches!(name, Name::Selector(ref s) if s.starts_with('.'))
                && i > 0
                && tokens[i - 1].kind == Kind::Punct(b'.')
            {
                tokens[i - 1].range.start..token.range.end
            } else {
                token.range.clone()
            };
            out.push(Occurrence {
                name,
                range,
                declaration,
            });
        }
    }
    out
}

fn at(occurrences: &[Occurrence], offset: usize) -> Option<&Occurrence> {
    occurrences
        .iter()
        .find(|o| o.range.start <= offset && offset <= o.range.end)
}

pub fn definition(text: &str, offset: usize, dialect: Dialect) -> Vec<Location> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let all = occurrences(text, dialect);
    let Some(here) = at(&all, offset) else {
        return Vec::new();
    };
    all.iter()
        .filter(|o| o.declaration && o.name == here.name)
        .map(|o| Location::in_text(None, text, o.range.clone()))
        .collect()
}

pub fn highlights(text: &str, offset: usize, dialect: Dialect) -> Vec<Range<usize>> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let all = occurrences(text, dialect);
    let Some(here) = at(&all, offset) else {
        return Vec::new();
    };
    all.iter()
        .filter(|o| o.name == here.name)
        .map(|o| o.range.clone())
        .collect()
}

// ---- Structure -----------------------------------------------------------------------------

pub fn symbols(text: &str, dialect: Dialect) -> Vec<Symbol> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let (tokens, _) = lexer::tokens(text, dialect);
    let mut out = Vec::new();
    for stmt in lexer::statements(text, &tokens, dialect) {
        let parts: Vec<&Token> = tokens[stmt.tokens.clone()]
            .iter()
            .filter(|t| !matches!(t.kind, Kind::Comment | Kind::Newline))
            .collect();
        let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
            continue;
        };
        let whole = first.range.start..last.range.end;
        let flat = |range: Range<usize>| -> String {
            text[range].split_whitespace().collect::<Vec<_>>().join(" ")
        };
        let symbol = match stmt.kind {
            Statement::Selector => Some(Symbol {
                name: flat(whole.clone()),
                kind: ItemKind::Class,
                detail: String::new(),
                range: whole,
                depth: stmt.depth,
            }),
            Statement::Declaration if matches!(first.kind, Kind::Variable | Kind::AtRule) => {
                let value = parts
                    .iter()
                    .position(|t| t.kind == Kind::Punct(b':'))
                    .and_then(|c| parts.get(c + 1))
                    .map(|t| flat(t.range.start..last.range.end))
                    .unwrap_or_default();
                Some(Symbol {
                    name: text[first.range.clone()].to_string(),
                    kind: ItemKind::Variable,
                    detail: value,
                    range: first.range.clone(),
                    depth: stmt.depth,
                })
            }
            Statement::AtRule => {
                let head = &text[first.range.clone()];
                let named = matches!(head, "@mixin" | "@function" | "=");
                if named && let Some(name) = parts.get(1) {
                    let rest = parts
                        .get(2)
                        .map(|t| flat(t.range.start..last.range.end))
                        .unwrap_or_default();
                    Some(Symbol {
                        name: text[name.range.clone()].to_string(),
                        kind: ItemKind::Function,
                        detail: rest,
                        range: name.range.clone(),
                        depth: stmt.depth,
                    })
                } else if stmt.opens {
                    Some(Symbol {
                        name: flat(whole.clone()),
                        kind: ItemKind::Module,
                        detail: String::new(),
                        range: whole,
                        depth: stmt.depth,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        out.extend(symbol);
    }
    out
}

// ---- Completion ----------------------------------------------------------------------------

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// Where the word at `offset` starts, with its sigil: `$`, `@`, `%`, `!`, `--`.
pub fn word_start(text: &str, offset: usize) -> usize {
    let mut from = offset;
    for (i, c) in text[..offset].char_indices().rev() {
        if word_char(c) {
            from = i;
        } else {
            if matches!(c, '$' | '@' | '%' | '!') {
                from = i;
            }
            break;
        }
    }
    from
}

/// What is being typed at the caret.
#[derive(Debug, PartialEq, Eq)]
enum Context {
    /// A property, or a nested rule, in a block.
    Property,
    /// The value of a property.
    Value(String),
    /// After `@`: an at-rule.
    AtRule,
    /// After `@include` or `+`: a mixin.
    Mixin,
    /// After `:` in a selector.
    Pseudo {
        element: bool,
    },
    /// A selector, outside of a block.
    Selector,
    Nothing,
}

fn context(text: &str, from: usize, dialect: Dialect) -> Context {
    let before = &text[..from];
    let (tokens, _) = lexer::tokens(before, dialect);
    // Inside a comment or a string that is not closed.
    if let Some(last) = tokens.last()
        && matches!(last.kind, Kind::Comment | Kind::String)
        && last.range.end == before.len()
    {
        let content = &before[last.range.clone()];
        let closed = if last.kind == Kind::Comment {
            content.ends_with("*/")
        } else {
            content.len() > 1 && content.ends_with(&content[..1])
        };
        if !closed || content.starts_with("//") {
            return Context::Nothing;
        }
    }
    let statements = lexer::statements(before, &tokens, dialect);
    // The statement the caret is in: the last one, if it does not end before the caret.
    let ends = |i: usize| {
        before[tokens[i].range.end..].trim().is_empty()
            && matches!(tokens[i].kind, Kind::Punct(b'{' | b'}' | b';'))
    };
    let current = statements
        .last()
        .filter(|s| s.tokens.end == tokens.len() && !(s.tokens.end > 0 && ends(s.tokens.end - 1)));
    let depth = {
        // How deep the caret is: the depth of a statement that would start here.
        let mut depth = 0usize;
        let mut interpolation = 0usize;
        for t in &tokens {
            match t.kind {
                Kind::Interpolation if before[t.range.clone()].starts_with('#') => {
                    interpolation += 1
                }
                Kind::Interpolation => interpolation = interpolation.saturating_sub(1),
                Kind::Punct(b'{') if interpolation == 0 => depth += 1,
                Kind::Punct(b'}') if interpolation == 0 => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if dialect == Dialect::Sass {
            let line_start = before.rfind('\n').map_or(0, |i| i + 1);
            usize::from(before[line_start..].starts_with([' ', '\t']))
        } else {
            depth
        }
    };
    let typed = &text[from..];
    let sigil = typed.chars().next();
    let Some(stmt) = current else {
        return if before.ends_with('@') || sigil == Some('@') {
            Context::AtRule
        } else if depth > 0 {
            Context::Property
        } else {
            Context::Selector
        };
    };
    let parts: Vec<&Token> = tokens[stmt.tokens.clone()]
        .iter()
        .filter(|t| !matches!(t.kind, Kind::Comment | Kind::Newline))
        .collect();
    let head = parts
        .first()
        .map(|t| &before[t.range.clone()])
        .unwrap_or("");
    if matches!(head, "@include" | "+") && parts.len() <= 1 {
        return Context::Mixin;
    }
    if head.starts_with('@') && parts.len() == 1 && parts[0].range.end == before.len() {
        return Context::AtRule;
    }
    let colon = parts.iter().position(|t| t.kind == Kind::Punct(b':'));
    match colon {
        Some(c) if parts.first().is_some_and(|t| t.kind == Kind::Ident) && c == 1 => {
            // Outside a block, `a:` starts a pseudo-class; inside, a value.
            if depth == 0 {
                let element = before.ends_with("::");
                Context::Pseudo { element }
            } else {
                Context::Value(head.to_string())
            }
        }
        Some(_) if before.ends_with(':') || before.ends_with("::") => Context::Pseudo {
            element: before.ends_with("::"),
        },
        _ if parts.len() == 1 && depth > 0 && parts[0].range.end == before.len() => {
            Context::Property
        }
        _ if depth == 0 => Context::Selector,
        _ if parts.iter().any(|t| t.kind == Kind::Punct(b':')) => Context::Value(head.to_string()),
        _ => Context::Nothing,
    }
}

pub fn complete(text: &str, offset: usize, explicit: bool, dialect: Dialect) -> Option<Completion> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let offset = offset.min(text.len());
    let from = word_start(text, offset);
    let typed = &text[from..offset];
    if typed.is_empty() && !explicit && !text[..from].ends_with([':', '@', '$']) {
        return None;
    }
    let mut items = Vec::new();
    let sheet = occurrences(text, dialect);
    let declared = |want: fn(&Name) -> Option<&str>| {
        let mut names: Vec<String> = sheet
            .iter()
            .filter(|o| o.declaration)
            .filter_map(|o| want(&o.name).map(str::to_string))
            .collect();
        names.sort();
        names.dedup();
        names
    };
    let variables = || {
        declared(|n| match n {
            Name::Variable(v) => Some(v),
            _ => None,
        })
    };
    // A variable, whatever the context: `$`, `--` and `@` (Less) say what is wanted.
    if typed.starts_with('$')
        || typed.starts_with("--")
        || dialect == Dialect::Less && typed.starts_with('@') && !at_statement_start(text, from)
    {
        for name in variables() {
            if name.starts_with(&typed[..1]) {
                items.push(CompletionItem::new(name, ItemKind::Variable).rank(10));
            }
        }
        return Some(Completion { from, items });
    }
    if typed.starts_with('!') {
        let words: &[&str] = if dialect == Dialect::Css {
            &["important"]
        } else {
            &["important", "default", "global", "optional"]
        };
        for word in words {
            items.push(CompletionItem::new(format!("!{word}"), ItemKind::Keyword));
        }
        return Some(Completion { from, items });
    }
    match context(text, from, dialect) {
        Context::AtRule => {
            let wanted = match dialect {
                Dialect::Css => 'c',
                Dialect::Less => 'l',
                Dialect::Scss | Dialect::Sass => 's',
            };
            for (name, dialects) in data::AT_RULES {
                if dialects.contains(wanted) || dialects.contains('c') && wanted != 'c' {
                    items.push(CompletionItem::new(format!("@{name}"), ItemKind::Keyword));
                }
            }
        }
        Context::Mixin => {
            for name in declared(|n| match n {
                Name::Mixin(m) => Some(m),
                _ => None,
            }) {
                items.push(CompletionItem::new(name, ItemKind::Function).rank(10));
            }
        }
        Context::Property => {
            let (open, close) = if dialect == Dialect::Sass {
                ("", "")
            } else {
                ("", ";")
            };
            for (name, _) in data::properties() {
                let insert = format!("{open}{name}: {close}");
                let caret = name.len() + 2;
                items.push(
                    CompletionItem::new(name, ItemKind::Property).insert(insert, Some(caret)),
                );
            }
            if dialect != Dialect::Css {
                for name in variables() {
                    items.push(CompletionItem::new(name, ItemKind::Variable).rank(60));
                }
            }
        }
        Context::Value(property) => {
            for value in data::values_of(&property) {
                items.push(CompletionItem::new(value, ItemKind::Keyword).rank(10));
            }
            for value in data::GLOBAL_VALUES {
                items.push(CompletionItem::new(*value, ItemKind::Keyword).rank(70));
            }
            for (name, arguments) in data::FUNCTIONS {
                items.push(
                    CompletionItem::new(*name, ItemKind::Function)
                        .detail(format!("({arguments})"))
                        .insert(format!("{name}()"), Some(name.len() + 1))
                        .rank(40),
                );
            }
            if property.starts_with("animation") {
                for name in declared(|n| match n {
                    Name::Keyframes(k) => Some(k),
                    _ => None,
                }) {
                    items.push(CompletionItem::new(name, ItemKind::Constant).rank(5));
                }
            }
            for name in variables() {
                items.push(CompletionItem::new(name, ItemKind::Variable).rank(30));
            }
            for name in declared(|n| match n {
                Name::Function(f) => Some(f),
                _ => None,
            }) {
                items.push(
                    CompletionItem::new(name.clone(), ItemKind::Function)
                        .insert(format!("{name}()"), Some(name.len() + 1))
                        .rank(20),
                );
            }
        }
        Context::Pseudo { element } => {
            let list = if element {
                data::PSEUDO_ELEMENTS
            } else {
                data::PSEUDO_CLASSES
            };
            for name in list {
                let (label, insert, caret) = match name.strip_suffix("()") {
                    Some(bare) => (bare, format!("{bare}()"), Some(bare.len() + 1)),
                    None => (*name, name.to_string(), None),
                };
                items.push(CompletionItem::new(label, ItemKind::Keyword).insert(insert, caret));
            }
        }
        Context::Selector => {
            for name in declared(|n| match n {
                Name::Selector(s) => Some(s),
                _ => None,
            }) {
                items.push(CompletionItem::new(name, ItemKind::Class));
            }
        }
        Context::Nothing => return None,
    }
    Some(Completion { from, items })
}

/// Nothing but blanks between the start of the statement and `from`.
fn at_statement_start(text: &str, from: usize) -> bool {
    text[..from]
        .trim_end()
        .chars()
        .next_back()
        .is_none_or(|c| matches!(c, '{' | '}' | ';'))
}

// ---- Reformat Code -------------------------------------------------------------------------

/// The text reindented by its blocks, a space after the colon of each declaration and before
/// the `{` of each rule, no blank at the end of the lines nor two blank lines in a row.
pub fn format(text: &str, dialect: Dialect, indent: &str) -> Result<String, String> {
    if dialect == Dialect::Sass {
        return Err(tr!("css.sass_no_formatter").into());
    }
    let (tokens, unclosed) = lexer::tokens(text, dialect);
    if !unclosed.is_empty() {
        return Err(tr!("css.cannot_format_errors").into());
    }
    let statements = lexer::statements(text, &tokens, dialect);
    // Insertions: (offset, text) and the spaces to replace.
    let mut edits: Vec<(Range<usize>, &str)> = Vec::new();
    for stmt in &statements {
        let parts: Vec<&Token> = tokens[stmt.tokens.clone()]
            .iter()
            .filter(|t| t.kind != Kind::Comment)
            .collect();
        if stmt.kind == Statement::Declaration
            && let Some(colon) = parts.iter().position(|t| t.kind == Kind::Punct(b':'))
            && let Some(value) = parts.get(colon + 1)
            && value.kind != Kind::Newline
        {
            let space = parts[colon].range.end..value.range.start;
            if !text[space.clone()].contains('\n') {
                edits.push((space, " "));
            }
        }
    }
    // `a{` becomes `a {`.
    for (i, token) in tokens.iter().enumerate() {
        if token.kind == Kind::Punct(b'{')
            && let Some(previous) = i.checked_sub(1).map(|p| &tokens[p])
            && previous.kind != Kind::Newline
        {
            let space = previous.range.end..token.range.start;
            if !text[space.clone()].contains('\n') {
                edits.push((space, " "));
            }
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    let mut spaced = String::with_capacity(text.len());
    let mut at = 0;
    for (range, with) in edits {
        if range.start < at {
            continue;
        }
        spaced.push_str(&text[at..range.start]);
        spaced.push_str(with);
        at = range.end;
    }
    spaced.push_str(&text[at..]);
    // Indentation by the depth of the braces, outside comments and strings.
    let (tokens, _) = lexer::tokens(&spaced, dialect);
    let mut starts = vec![0];
    starts.extend(spaced.match_indices('\n').map(|(i, _)| i + 1));
    let line_of = |offset: usize| starts.partition_point(|&s| s <= offset) - 1;
    // Per line: the `}` it starts with, the `{` and `}` after them, and whether it is inside a
    // comment that started on a line before.
    let mut leading = vec![0i64; starts.len()];
    let mut net = vec![0i64; starts.len()];
    let mut started = vec![false; starts.len()];
    let mut in_comment = vec![false; starts.len()];
    for token in &tokens {
        let line = line_of(token.range.start);
        match token.kind {
            Kind::Comment => {
                let last = line_of(token.range.end.saturating_sub(1).max(token.range.start));
                for slot in &mut in_comment[line + 1..=last] {
                    *slot = true;
                }
                started[line] = true;
            }
            Kind::Newline => {}
            Kind::Punct(b'}') if !started[line] => leading[line] += 1,
            Kind::Punct(b'{') => {
                net[line] += 1;
                started[line] = true;
            }
            Kind::Punct(b'}') => net[line] -= 1,
            _ => started[line] = true,
        }
    }
    let mut out = String::with_capacity(spaced.len());
    let mut depth = 0i64;
    let mut blank = 0;
    for (line, &start) in starts.iter().enumerate() {
        let end = starts.get(line + 1).map_or(spaced.len(), |&s| s - 1);
        let raw = spaced[start..end].trim_end();
        let own = (depth - leading[line]).max(0);
        depth = (own + net[line]).max(0);
        if raw.trim().is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
            out.push('\n');
            continue;
        }
        blank = 0;
        if in_comment[line] {
            out.push_str(raw);
        } else {
            out.push_str(&indent.repeat(own as usize));
            out.push_str(raw.trim_start());
        }
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    Ok(out)
}

/// The indentation of a file: what its first indented line uses, two spaces otherwise.
pub fn indent_of(text: &str) -> String {
    text.lines()
        .map(|l| &l[..l.len() - l.trim_start().len()])
        .find(|i| !i.is_empty())
        .map_or_else(
            || "  ".to_string(),
            |i| {
                if i.starts_with('\t') {
                    "\t".to_string()
                } else {
                    " ".repeat(i.len().min(8))
                }
            },
        )
}
