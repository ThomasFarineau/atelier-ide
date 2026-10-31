//! Reformat Code and Minify for style sheets with braces (CSS, SCSS, Less), from the tokens:
//! whatever the text looks like, one line per declaration and per selector, the blocks
//! indented, the spaces where they go; or the smallest text that means the same.

use super::lexer::{self, Dialect, Kind, Token};

/// A statement as it is printed: its tokens (no comment, no line break) and what ends it.
struct Piece<'a> {
    tokens: Vec<&'a Token>,
}

/// Whether a space goes between two tokens of a statement: where the text had one, after a
/// comma, around the colon of a declaration, never inside parentheses at their ends.
fn spaced(
    text: &str,
    a: &Token,
    b: &Token,
    header: bool,
    at_rule: bool,
    colon: Option<usize>,
    b_index: usize,
) -> bool {
    let between = &text[a.range.end..b.range.start];
    let had = !between.is_empty();
    match (a.kind, b.kind) {
        (Kind::Punct(b'('), _) | (_, Kind::Punct(b')')) => false,
        (_, Kind::Punct(b',' | b';')) => false,
        (Kind::Punct(b','), _) => true,
        (Kind::Interpolation, _) if text[a.range.clone()].starts_with('#') => false,
        (_, Kind::Interpolation) if !text[b.range.clone()].starts_with('#') => false,
        _ if colon == Some(b_index) => false,
        _ if colon.is_some_and(|c| c + 1 == b_index) => true,
        // `(min-width: 600px)` in the condition of an at-rule.
        (Kind::Punct(b':'), _) if at_rule => true,
        (_, Kind::Punct(b':')) if at_rule => false,
        // Combinators of selectors.
        (Kind::Punct(b'>' | b'~'), _) | (_, Kind::Punct(b'>' | b'~')) if header => true,
        (Kind::Punct(b'+'), _) | (_, Kind::Punct(b'+')) if header => true,
        _ => had,
    }
}

/// The index (in `tokens`) of the colon of a declaration: the first one outside parentheses,
/// after a property or a variable.
pub(super) fn declaration_colon(text: &str, tokens: &[&Token]) -> Option<usize> {
    let first = tokens.first()?;
    if !matches!(
        first.kind,
        Kind::Ident | Kind::Variable | Kind::AtRule | Kind::Interpolation
    ) {
        return None;
    }
    if first.kind == Kind::AtRule && text[first.range.clone()].starts_with("@media") {
        return None;
    }
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            Kind::Punct(b'(') => depth += 1,
            Kind::Punct(b')') => depth = depth.saturating_sub(1),
            Kind::Punct(b':') if depth == 0 => {
                // `@include a(…)`: an at-rule without a colon of its own.
                if first.kind == Kind::AtRule && i != 1 {
                    return None;
                }
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

/// The text of a statement on one line, its spaces made right.
fn print(text: &str, piece: &Piece, header: bool) -> String {
    let tokens = &piece.tokens;
    let colon = if header {
        None
    } else {
        declaration_colon(text, tokens)
    };
    let at_rule = is_at_rule(text, tokens);
    let mut out = String::new();
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Punct(b'(') => depth += 1,
            Kind::Punct(b')') => depth = depth.saturating_sub(1),
            _ => {}
        }
        if i > 0 && spaced(text, tokens[i - 1], token, header, at_rule, colon, i) {
            out.push(' ');
        }
        out.push_str(&text[token.range.clone()]);
        // A list of selectors: one per line.
        if header && token.kind == Kind::Punct(b',') && depth == 0 && !at_rule {
            out.push('\n');
        }
    }
    // `a ,\n b`: no space before the line break.
    out.replace(" \n", "\n").replace("\n ", "\n")
}

fn is_at_rule(text: &str, tokens: &[&Token]) -> bool {
    tokens
        .first()
        .is_some_and(|t| t.kind == Kind::AtRule && text[t.range.clone()].starts_with('@'))
}

/// The number of line breaks between two offsets.
fn breaks(text: &str, from: usize, to: usize) -> usize {
    text[from..to].bytes().filter(|b| *b == b'\n').count()
}

pub fn reformat(text: &str, dialect: Dialect, indent: &str) -> Result<String, String> {
    if dialect == Dialect::Sass {
        return Err(tr!("css.sass_no_formatter").into());
    }
    let (tokens, unclosed) = lexer::tokens(text, dialect);
    if !unclosed.is_empty() {
        return Err(tr!("css.cannot_format_errors").into());
    }
    let mut out = String::new();
    let mut depth = 0usize;
    let mut current: Vec<&Token> = Vec::new();
    let mut parens = 0usize;
    let mut interpolation = 0usize;
    // Where the last thing printed ended in the text, for the blank lines and the comments
    // at the end of a line.
    let mut last_end = 0usize;
    let push_line = |out: &mut String, depth: usize, line: &str| {
        for (n, part) in line.split('\n').enumerate() {
            let _ = n;
            out.push_str(&indent.repeat(depth));
            out.push_str(part.trim());
            out.push('\n');
        }
    };
    // A blank line kept from the text, one at most, never after an opening brace.
    let blank = |out: &mut String, text: &str, from: usize, to: usize| {
        if breaks(text, from, to) >= 2
            && !out.is_empty()
            && !out.ends_with("{\n")
            && !out.ends_with("\n\n")
        {
            out.push('\n');
        }
    };
    for token in &tokens {
        match token.kind {
            Kind::Newline => continue,
            Kind::Comment => {
                if current.is_empty() {
                    let same_line = last_end > 0 && breaks(text, last_end, token.range.start) == 0;
                    if same_line && out.ends_with('\n') {
                        out.pop();
                        out.push(' ');
                        out.push_str(&text[token.range.clone()]);
                        out.push('\n');
                    } else {
                        blank(&mut out, text, last_end, token.range.start);
                        out.push_str(&indent.repeat(depth));
                        out.push_str(&text[token.range.clone()]);
                        out.push('\n');
                    }
                    last_end = token.range.end;
                }
                // A comment inside a statement is dropped from its line but kept before it.
                else {
                    out.push_str(&indent.repeat(depth));
                    out.push_str(&text[token.range.clone()]);
                    out.push('\n');
                }
                continue;
            }
            Kind::Interpolation if text[token.range.clone()].starts_with('#') => interpolation += 1,
            Kind::Interpolation => interpolation = interpolation.saturating_sub(1),
            Kind::Punct(b'(') => parens += 1,
            Kind::Punct(b')') => parens = parens.saturating_sub(1),
            _ => {}
        }
        let ends = interpolation == 0 && matches!(token.kind, Kind::Punct(b'{' | b'}'))
            || interpolation == 0 && parens == 0 && token.kind == Kind::Punct(b';');
        if !ends {
            if current.is_empty() {
                blank(&mut out, text, last_end, token.range.start);
            }
            current.push(token);
            continue;
        }
        let piece = Piece {
            tokens: std::mem::take(&mut current),
        };
        match token.kind {
            Kind::Punct(b'{') => {
                let header = print(text, &piece, true);
                let line = if header.is_empty() {
                    "{".to_string()
                } else {
                    format!("{header} {{")
                };
                push_line(&mut out, depth, &line);
                depth += 1;
                parens = 0;
            }
            Kind::Punct(b';') => {
                if !piece.tokens.is_empty() {
                    push_line(&mut out, depth, &format!("{};", print(text, &piece, false)));
                }
            }
            _ => {
                // `}`: the last declaration of a block gets its semicolon.
                if !piece.tokens.is_empty() {
                    push_line(&mut out, depth, &format!("{};", print(text, &piece, false)));
                }
                depth = depth.saturating_sub(1);
                push_line(&mut out, depth, "}");
                parens = 0;
                // Rules at the top are apart.
                if depth == 0 {
                    out.push('\n');
                }
            }
        }
        last_end = token.range.end;
    }
    if !current.is_empty() {
        let piece = Piece { tokens: current };
        push_line(&mut out, depth, &print(text, &piece, false));
    }
    // One blank line at most, none at the end.
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    let trimmed = out.trim_end_matches('\n');
    Ok(if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    })
}

/// Whether the space the text has between two tokens must stay: not after what opens or
/// separates, not before what closes or separates, never around the colon of a declaration.
/// `a :hover` (a descendant) is not `a:hover`, `and (` needs its space, `calc(1px + 2px)` too.
fn needs_space(a: &Token, b: &Token, declaration_colon: bool) -> bool {
    if declaration_colon || a.kind == Kind::Comment {
        return false;
    }
    let free_after = matches!(
        a.kind,
        Kind::Punct(b'{' | b'}' | b';' | b',' | b'>' | b'~' | b'(' | b':')
    );
    let free_before = matches!(
        b.kind,
        Kind::Punct(b'{' | b'}' | b';' | b',' | b'>' | b'~' | b')')
    );
    !(free_after || free_before)
}

pub fn minify(text: &str, dialect: Dialect) -> Result<String, String> {
    if dialect == Dialect::Sass {
        return Err(tr!("css.sass_no_minifier").into());
    }
    let (tokens, unclosed) = lexer::tokens(text, dialect);
    if !unclosed.is_empty() {
        return Err(tr!("css.cannot_format_errors").into());
    }
    // Comments go, but `/*! … */` (licenses).
    let kept: Vec<&Token> = tokens
        .iter()
        .filter(|t| match t.kind {
            Kind::Newline => false,
            Kind::Comment => text[t.range.clone()].starts_with("/*!"),
            _ => true,
        })
        .collect();
    // The colons of declarations, which need no space around them.
    let statements = lexer::statements(text, &tokens, dialect);
    let mut colons = std::collections::HashSet::new();
    for stmt in &statements {
        if stmt.kind == lexer::Statement::Declaration {
            let parts: Vec<&Token> = tokens[stmt.tokens.clone()]
                .iter()
                .filter(|t| !matches!(t.kind, Kind::Comment | Kind::Newline))
                .collect();
            if let Some(c) = declaration_colon(text, &parts) {
                colons.insert(parts[c].range.start);
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    for (i, token) in kept.iter().enumerate() {
        // The last `;` of a block.
        if token.kind == Kind::Punct(b';')
            && kept.get(i + 1).is_some_and(|n| n.kind == Kind::Punct(b'}'))
        {
            continue;
        }
        if i > 0 {
            let previous = kept[i - 1];
            // Between them, nothing but blanks (comments that went count as a space).
            let had = previous.range.end < token.range.start;
            let around_colon =
                colons.contains(&token.range.start) || colons.contains(&previous.range.start);
            if had && needs_space(previous, token, around_colon) {
                out.push(' ');
            }
        }
        out.push_str(&text[token.range.clone()]);
    }
    Ok(out)
}
