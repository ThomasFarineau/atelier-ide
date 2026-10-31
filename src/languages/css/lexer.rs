//! The tokens of a style sheet, and what each one is in it (a selector, a property, a
//! value…), for CSS, SCSS, Less and Sass (the indented syntax) alike.
//!
//! The four are close enough for one lexer: what one of them does not have does not appear in
//! its files. What differs is told by the [`Dialect`]: `//` starts a comment except in CSS, and
//! in Sass a line ends a statement as `;` does elsewhere.

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect {
    Css,
    Scss,
    Less,
    /// The indented syntax: no braces, no semicolons.
    Sass,
}

impl Dialect {
    /// The dialect of a language as `project::language` names it.
    pub fn of(language: &str) -> Self {
        match language {
            "scss" => Self::Scss,
            "less" => Self::Less,
            "sass" => Self::Sass,
            _ => Self::Css,
        }
    }

    /// The dialect of a file from its extension.
    pub fn of_path(path: Option<&std::path::Path>) -> Self {
        let extension = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        Self::of(&extension)
    }

    /// The dialect of a text whose file is not known: indented when it has no braces.
    pub fn guess(text: &str) -> Self {
        if text.contains('{') || text.contains(';') {
            Self::Scss
        } else if text.lines().any(|l| l.starts_with([' ', '\t'])) {
            Self::Sass
        } else {
            Self::Scss
        }
    }

    pub fn line_comments(self) -> bool {
        self != Self::Css
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Comment,
    String,
    /// A number with its unit: `12px`, `1.5em`, `50%`.
    Number,
    /// `#abc`: a color in a value, an id in a selector.
    Hash,
    /// `$name`, `--name`.
    Variable,
    /// `@media`; also a variable of Less (`@color`).
    AtRule,
    Ident,
    /// A name followed by `(`: `rgb`, `calc`, `darken`, `:not`.
    Function,
    /// `%name`, the placeholder selector of Sass.
    Placeholder,
    /// `!important`, `!default`.
    Bang,
    /// `#{` and its `}`: code inside a selector, a name or a string.
    Interpolation,
    /// One character: `{ } ( ) [ ] : ; , . & > + ~ * = /`.
    Punct(u8),
    /// The end of a line, which ends a statement in Sass.
    Newline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
}

/// What a lexer found wrong: a comment or a string not closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unclosed {
    pub range: Range<usize>,
    pub comment: bool,
}

fn ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80 || b == b'\\'
}

fn ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b >= 0x80 || b == b'\\'
}

/// The end of the name that starts at `i`.
fn name_end(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && ident_char(bytes[i]) {
        // An escape takes the character after it.
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
        }
        i += 1;
    }
    i
}

/// Whether a name starts at `i`: a letter, or `-` followed by a letter or another `-`.
fn name_at(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i) {
        Some(&b) if ident_start(b) => true,
        Some(b'-') => bytes
            .get(i + 1)
            .is_some_and(|&n| ident_start(n) || n == b'-'),
        _ => false,
    }
}

pub fn tokens(text: &str, dialect: Dialect) -> (Vec<Token>, Vec<Unclosed>) {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut unclosed = Vec::new();
    // What each open `{` is: a block, or an interpolation `#{`.
    let mut braces: Vec<bool> = Vec::new();
    let mut i = 0;
    let push = |out: &mut Vec<Token>, kind, range| out.push(Token { kind, range });
    while i < bytes.len() {
        let b = bytes[i];
        let start = i;
        match b {
            b'\n' => {
                push(&mut out, Kind::Newline, i..i + 1);
                i += 1;
            }
            b' ' | b'\t' | b'\r' | b'\x0c' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                match text[i + 2..].find("*/") {
                    Some(end) => i += 2 + end + 2,
                    None => {
                        i = bytes.len();
                        unclosed.push(Unclosed {
                            range: start..start + 2,
                            comment: true,
                        });
                    }
                }
                push(&mut out, Kind::Comment, start..i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') && dialect.line_comments() => {
                i = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                push(&mut out, Kind::Comment, start..i);
            }
            b'"' | b'\'' => {
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'\n' => break,
                        c if c == b => {
                            i += 1;
                            closed = true;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                i = i.min(bytes.len());
                if !closed {
                    unclosed.push(Unclosed {
                        range: start..i,
                        comment: false,
                    });
                }
                push(&mut out, Kind::String, start..i);
            }
            b'0'..=b'9' => i = number(bytes, i, &mut out),
            b'.' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i = number(bytes, i, &mut out);
            }
            // A sign that belongs to the number: `-2px`, not `a - 2` nor `a-2`.
            b'-' | b'+'
                if bytes
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == b'.')
                    && !out.last().is_some_and(|t: &Token| {
                        t.range.end == i
                            && matches!(t.kind, Kind::Ident | Kind::Number | Kind::Variable)
                    }) =>
            {
                i = number(bytes, i, &mut out);
            }
            b'#' if bytes.get(i + 1) == Some(&b'{') => {
                braces.push(true);
                i += 2;
                push(&mut out, Kind::Interpolation, start..i);
            }
            b'#' => {
                i = name_end(bytes, i + 1);
                push(&mut out, Kind::Hash, start..i);
            }
            b'$' if name_at(bytes, i + 1) => {
                i = name_end(bytes, i + 1);
                push(&mut out, Kind::Variable, start..i);
            }
            b'@' if name_at(bytes, i + 1) => {
                i = name_end(bytes, i + 1);
                push(&mut out, Kind::AtRule, start..i);
            }
            b'!' if bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) => {
                i = name_end(bytes, i + 1);
                push(&mut out, Kind::Bang, start..i);
            }
            b'%' if name_at(bytes, i + 1) => {
                i = name_end(bytes, i + 1);
                push(&mut out, Kind::Placeholder, start..i);
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i = name_end(bytes, i + 2);
                push(&mut out, Kind::Variable, start..i);
            }
            _ if name_at(bytes, i) => {
                i = name_end(bytes, i);
                if bytes.get(i) == Some(&b'(') {
                    push(&mut out, Kind::Function, start..i);
                    // `url(…)` without quotes: what is inside is one string.
                    let name = &text[start..i];
                    if name.eq_ignore_ascii_case("url") {
                        let open = i;
                        push(&mut out, Kind::Punct(b'('), open..open + 1);
                        let inner = open + 1;
                        let quoted = text[inner..].trim_start().starts_with(['"', '\'']);
                        if !quoted && let Some(close) = text[inner..].find([')', '\n']) {
                            let end = inner + close;
                            if end > inner {
                                push(&mut out, Kind::String, inner..end);
                            }
                            i = end;
                        } else {
                            i = inner;
                        }
                    }
                } else {
                    push(&mut out, Kind::Ident, start..i);
                }
            }
            b'{' => {
                braces.push(false);
                i += 1;
                push(&mut out, Kind::Punct(b), start..i);
            }
            b'}' => {
                i += 1;
                let kind = if braces.pop() == Some(true) {
                    Kind::Interpolation
                } else {
                    Kind::Punct(b)
                };
                push(&mut out, kind, start..i);
            }
            _ if b.is_ascii() => {
                i += 1;
                push(&mut out, Kind::Punct(b), start..i);
            }
            // Another character outside a name: skipped whole.
            _ => {
                i += 1;
                while i < bytes.len() && !text.is_char_boundary(i) {
                    i += 1;
                }
            }
        }
    }
    (out, unclosed)
}

/// A number from `i`, its sign, its decimals, its exponent and its unit. Where it ends.
fn number(bytes: &[u8], mut i: usize, out: &mut Vec<Token>) -> usize {
    let start = i;
    if matches!(bytes[i], b'-' | b'+') {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len()
        && matches!(bytes[i], b'e' | b'E')
        && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
    {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len() && bytes[i] == b'%' {
        i += 1;
    } else if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i = name_end(bytes, i);
    }
    out.push(Token {
        kind: Kind::Number,
        range: start..i,
    });
    i
}

/// What a token is in the style sheet, for its color and for the analysis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Comment,
    String,
    Number,
    /// `@media`, `!important`, `&`, `and`.
    Keyword,
    Property,
    Variable,
    /// A tag name in a selector.
    Tag,
    /// `.class`, `#id`, `%placeholder`.
    Class,
    /// `:hover`, `::before`.
    Pseudo,
    /// A function, a mixin.
    Function,
    /// `#{` `}`.
    Interpolation,
    /// The name of an attribute in a selector: `[type]`.
    Attribute,
    /// What stays the color of the text.
    Text,
}

/// What a statement is, once its end is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Statement {
    /// `a.b:hover {`
    Selector,
    /// `color: red;`, `$x: 1;`
    Declaration,
    /// `@media screen {`, `@include x;`
    AtRule,
    /// Anything else (an empty statement, a value alone).
    Other,
}

/// A statement: its tokens (indices in the token list), what it is and how deep it is.
#[derive(Clone, Debug)]
pub struct Stmt {
    pub tokens: Range<usize>,
    pub kind: Statement,
    /// Blocks it is in: braces, or indentation in Sass.
    pub depth: usize,
    /// It opens a block (`{`, or an indented line after it in Sass).
    pub opens: bool,
}

fn meaningful(kind: Kind) -> bool {
    !matches!(kind, Kind::Comment | Kind::Newline)
}

/// The width of the indentation of the line that starts at `start`.
fn indentation(text: &str, start: usize) -> usize {
    text[start..]
        .bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .map(|b| if b == b'\t' { 4 } else { 1 })
        .sum()
}

/// The statements of a token list.
pub fn statements(text: &str, tokens: &[Token], dialect: Dialect) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::new();
    let mut depth = 0usize;
    // Sass: the indentation of each open block.
    let mut indents: Vec<usize> = Vec::new();
    let mut start = 0usize;
    let mut parens = 0usize;
    let mut interpolation = 0usize;
    let close = |out: &mut Vec<Stmt>, range: Range<usize>, depth: usize, opens: bool| {
        if tokens[range.clone()].iter().any(|t| meaningful(t.kind)) {
            let kind = classify(text, &tokens[range.clone()], dialect, opens);
            out.push(Stmt {
                tokens: range,
                kind,
                depth,
                opens,
            });
        }
    };
    for (i, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Interpolation if text[token.range.clone()].starts_with('#') => interpolation += 1,
            Kind::Interpolation => interpolation = interpolation.saturating_sub(1),
            Kind::Punct(b'(') => parens += 1,
            Kind::Punct(b')') => parens = parens.saturating_sub(1),
            _ => {}
        }
        if interpolation > 0 {
            continue;
        }
        match token.kind {
            Kind::Punct(b'{') => {
                close(&mut out, start..i, depth, true);
                depth += 1;
                parens = 0;
                start = i + 1;
            }
            Kind::Punct(b'}') => {
                close(&mut out, start..i, depth, false);
                depth = depth.saturating_sub(1);
                parens = 0;
                start = i + 1;
            }
            Kind::Punct(b';') if parens == 0 => {
                close(&mut out, start..i, depth, false);
                start = i + 1;
            }
            Kind::Newline if dialect == Dialect::Sass && parens == 0 => {
                close(&mut out, start..i, depth, false);
                start = i + 1;
                // The next line, by its indentation, says how deep it is.
                let line = token.range.end;
                let rest = &text[line..];
                let blank = rest.split('\n').next().is_none_or(|l| l.trim().is_empty());
                if !blank {
                    let width = indentation(text, line);
                    let deeper = width > indents.last().copied().unwrap_or(0);
                    if deeper && let Some(last) = out.last_mut() {
                        // The line before opens a block.
                        if last.depth == depth && !last.opens {
                            last.opens = true;
                            last.kind = classify(text, &tokens[last.tokens.clone()], dialect, true);
                        }
                        indents.push(width);
                        depth += 1;
                    } else {
                        while indents.last().is_some_and(|&w| width < w) {
                            indents.pop();
                            depth = depth.saturating_sub(1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    close(&mut out, start..tokens.len(), depth, false);
    out
}

/// What a statement is, from its tokens and whether it opens a block.
fn classify(text: &str, tokens: &[Token], dialect: Dialect, opens: bool) -> Statement {
    let mut it = tokens.iter().filter(|t| meaningful(t.kind));
    let Some(first) = it.next() else {
        return Statement::Other;
    };
    let second = it.next();
    let colon_after = second.is_some_and(|t| t.kind == Kind::Punct(b':'));
    match first.kind {
        // A variable of Less: `@color: red`.
        Kind::AtRule if dialect == Dialect::Less && colon_after => Statement::Declaration,
        Kind::AtRule => Statement::AtRule,
        // The mixins of Sass: `=name` and `+name`.
        Kind::Punct(b'=' | b'+') if dialect == Dialect::Sass => Statement::AtRule,
        Kind::Variable if colon_after => Statement::Declaration,
        Kind::Ident | Kind::Interpolation if colon_after => {
            let colon = second.expect("checked");
            let after = text[colon.range.end..].chars().next();
            // `a:hover {` is a selector, `font: 12px {` a nested property of SCSS.
            if opens {
                let spaced = after.is_some_and(char::is_whitespace);
                if spaced && dialect != Dialect::Css {
                    Statement::Declaration
                } else {
                    Statement::Selector
                }
            } else if dialect == Dialect::Sass {
                // `a:hover` alone on its line is a selector whose block follows; a property
                // has a space after its colon.
                if after.is_none_or(char::is_whitespace) {
                    Statement::Declaration
                } else {
                    Statement::Selector
                }
            } else {
                Statement::Declaration
            }
        }
        _ if opens => Statement::Selector,
        _ => Statement::Other,
    }
}

/// The role of each token (in the same order).
pub fn roles(text: &str, tokens: &[Token], dialect: Dialect) -> Vec<Role> {
    let mut roles: Vec<Role> = tokens
        .iter()
        .map(|t| match t.kind {
            Kind::Comment => Role::Comment,
            Kind::String => Role::String,
            Kind::Number => Role::Number,
            Kind::Interpolation => Role::Interpolation,
            Kind::Variable => Role::Variable,
            Kind::Bang => Role::Keyword,
            _ => Role::Text,
        })
        .collect();
    for stmt in statements(text, tokens, dialect) {
        let range = stmt.tokens.clone();
        match stmt.kind {
            Statement::Selector => selector_roles(text, tokens, range, &mut roles),
            Statement::Declaration => {
                let mut seen_colon = false;
                for i in range {
                    let token = &tokens[i];
                    if !seen_colon {
                        match token.kind {
                            Kind::Punct(b':') => seen_colon = true,
                            Kind::Ident => roles[i] = Role::Property,
                            Kind::AtRule => roles[i] = Role::Variable,
                            _ => {}
                        }
                    } else {
                        value_role(text, token, &mut roles[i], dialect);
                    }
                }
            }
            Statement::AtRule => {
                let mut name_next = false;
                for i in range {
                    let token = &tokens[i];
                    let word = &text[token.range.clone()];
                    match token.kind {
                        // A feature of a query: `(min-width: 600px)`.
                        Kind::Ident
                            if tokens
                                .get(i + 1)
                                .is_some_and(|n| n.kind == Kind::Punct(b':')) =>
                        {
                            roles[i] = Role::Property;
                        }
                        Kind::AtRule if is_first(tokens, &stmt, i) => {
                            roles[i] = Role::Keyword;
                            name_next = matches!(
                                word,
                                "@mixin" | "@include" | "@function" | "@keyframes" | "@extend"
                            ) || word.ends_with("keyframes");
                        }
                        Kind::Punct(b'=' | b'+') if is_first(tokens, &stmt, i) => {
                            roles[i] = Role::Keyword;
                            name_next = true;
                        }
                        Kind::Ident | Kind::Function | Kind::Placeholder if name_next => {
                            roles[i] = if token.kind == Kind::Placeholder {
                                Role::Class
                            } else {
                                Role::Function
                            };
                            name_next = false;
                        }
                        Kind::Ident
                            if matches!(
                                word,
                                "and"
                                    | "or"
                                    | "not"
                                    | "only"
                                    | "from"
                                    | "through"
                                    | "to"
                                    | "in"
                                    | "if"
                                    | "else"
                                    | "as"
                                    | "with"
                            ) =>
                        {
                            roles[i] = Role::Keyword;
                        }
                        Kind::Punct(b'.' | b'&') => {}
                        _ => value_role(text, token, &mut roles[i], dialect),
                    }
                    if token.kind == Kind::Hash && name_next {
                        name_next = false;
                    }
                }
            }
            Statement::Other => {
                for i in range {
                    value_role(text, &tokens[i], &mut roles[i], dialect);
                }
            }
        }
    }
    roles
}

fn is_first(tokens: &[Token], stmt: &Stmt, i: usize) -> bool {
    tokens[stmt.tokens.start..i]
        .iter()
        .all(|t| !meaningful(t.kind))
}

/// The role of a token of a value: functions, colors, the variables of Less.
fn value_role(text: &str, token: &Token, role: &mut Role, dialect: Dialect) {
    match token.kind {
        Kind::Function => *role = Role::Function,
        Kind::Hash if is_color(&text[token.range.clone()]) => *role = Role::Number,
        Kind::AtRule if dialect == Dialect::Less => *role = Role::Variable,
        Kind::AtRule => *role = Role::Keyword,
        _ => {}
    }
}

/// `#fff`, `#ffff`, `#ffffff`, `#ffffff80`.
pub fn is_color(hash: &str) -> bool {
    let digits = &hash[1..];
    matches!(digits.len(), 3 | 4 | 6 | 8) && digits.bytes().all(|b| b.is_ascii_hexdigit())
}

fn selector_roles(text: &str, tokens: &[Token], range: Range<usize>, roles: &mut [Role]) {
    let mut bracket = false;
    for i in range {
        let token = &tokens[i];
        let previous = i.checked_sub(1).map(|p| &tokens[p]);
        let glued = |kind: Kind| {
            previous.is_some_and(|p| p.kind == kind && p.range.end == token.range.start)
        };
        match token.kind {
            Kind::Punct(b'[') => bracket = true,
            Kind::Punct(b']') => bracket = false,
            Kind::Ident if bracket => roles[i] = Role::Attribute,
            Kind::Ident | Kind::Function if glued(Kind::Punct(b':')) => {
                roles[i] = Role::Pseudo;
                roles[i - 1] = Role::Pseudo;
                // `::before`
                if i >= 2
                    && tokens[i - 2].kind == Kind::Punct(b':')
                    && tokens[i - 2].range.end == tokens[i - 1].range.start
                {
                    roles[i - 2] = Role::Pseudo;
                }
            }
            Kind::Ident | Kind::Function | Kind::Interpolation if glued(Kind::Punct(b'.')) => {
                if token.kind != Kind::Interpolation {
                    roles[i] = Role::Class;
                }
                roles[i - 1] = Role::Class;
            }
            Kind::Ident if text[token.range.clone()].ends_with('%') => {}
            Kind::Ident => {
                // A part of a name after `&` (`&-item`) is the name of a class.
                roles[i] = if glued(Kind::Punct(b'&')) {
                    Role::Class
                } else {
                    Role::Tag
                };
            }
            Kind::Hash | Kind::Placeholder => roles[i] = Role::Class,
            Kind::Punct(b'&') => roles[i] = Role::Keyword,
            // `50%` in the selectors of `@keyframes`.
            Kind::Number => roles[i] = Role::Number,
            Kind::Function => roles[i] = Role::Function,
            _ => {}
        }
    }
}

/// The colored ranges of each line: what is not the color of the text.
pub fn line_roles(text: &str, dialect: Dialect) -> Vec<Vec<(Range<usize>, Role)>> {
    let (tokens, _) = tokens(text, dialect);
    let roles = roles(text, &tokens, dialect);
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let mut lines: Vec<Vec<(Range<usize>, Role)>> = vec![Vec::new(); starts.len()];
    for (token, role) in tokens.iter().zip(roles) {
        if role == Role::Text || token.kind == Kind::Newline {
            continue;
        }
        // A comment or a string over several lines is cut at each line.
        let mut at = token.range.start;
        while at < token.range.end {
            let line = starts.partition_point(|&s| s <= at) - 1;
            let line_end = starts
                .get(line + 1)
                .map_or(text.len(), |&next| next - 1)
                .min(token.range.end);
            if line_end > at {
                lines[line].push((at - starts[line]..line_end - starts[line], role));
            }
            at = line_end + 1;
        }
    }
    lines
}
