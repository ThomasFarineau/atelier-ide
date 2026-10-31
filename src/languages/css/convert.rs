//! A style sheet written again in another dialect: CSS, SCSS, Less or Sass into SCSS, Less or
//! Sass. The sheet is read into a tree (rules, declarations, at-rules, comments) and printed in
//! the other syntax: the variables (`$x`, `@x`), the mixins (`@mixin`, `.m()`, `=m`), their
//! uses, `@extend` and the interpolations follow. From CSS, the rules that follow each other
//! are nested (`.a .b`, `.a:hover` into `.a { .b {} &:hover {} }`).
//!
//! What has no counterpart (`@if` and `@each` of Sass in Less) is copied as it is.

use super::{
    format::declaration_colon,
    lexer::{self, Dialect, Kind, Role, Statement, Token},
};

/// A node of the sheet; the numbers are indices of tokens.
#[derive(Clone, Debug)]
enum Node {
    Comment(usize),
    /// `name: value`.
    Declaration {
        name: Vec<usize>,
        value: Vec<usize>,
    },
    /// A selector or an at-rule, and its block.
    Block {
        header: Vec<usize>,
        children: Vec<Node>,
    },
    /// A statement without a block: `@include x`, `@import "a"`, `.m();`.
    Line(Vec<usize>),
    /// A rule made by the nesting of CSS: its selector is text.
    Nested {
        selector: String,
        children: Vec<Node>,
    },
}

struct Sheet<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    roles: Vec<Role>,
    from: Dialect,
    to: Dialect,
}

fn parse(sheet: &Sheet) -> Vec<Node> {
    let text = sheet.text;
    let tokens = &sheet.tokens;
    let statements = lexer::statements(text, tokens, sheet.from);
    // Open blocks: their header and what they hold so far.
    let mut stack: Vec<(Vec<usize>, Vec<Node>)> = vec![(Vec::new(), Vec::new())];
    let comments: Vec<usize> = (0..tokens.len())
        .filter(|&i| tokens[i].kind == Kind::Comment)
        .collect();
    let mut next_comment = 0;
    let close_to = |stack: &mut Vec<(Vec<usize>, Vec<Node>)>, depth: usize| {
        while stack.len() > depth + 1 {
            let (header, children) = stack.pop().expect("deeper than the top");
            stack
                .last_mut()
                .expect("the top stays")
                .1
                .push(Node::Block { header, children });
        }
    };
    for stmt in &statements {
        let parts: Vec<usize> = stmt
            .tokens
            .clone()
            .filter(|&i| !matches!(tokens[i].kind, Kind::Comment | Kind::Newline))
            .collect();
        let Some(&first) = parts.first() else {
            continue;
        };
        close_to(&mut stack, stmt.depth);
        // The comments before the statement, at its depth.
        while next_comment < comments.len()
            && tokens[comments[next_comment]].range.start < tokens[first].range.start
        {
            stack
                .last_mut()
                .expect("the top stays")
                .1
                .push(Node::Comment(comments[next_comment]));
            next_comment += 1;
        }
        if stmt.opens {
            stack.push((parts, Vec::new()));
            continue;
        }
        let refs: Vec<&Token> = parts.iter().map(|&i| &tokens[i]).collect();
        let node = match declaration_colon(text, &refs) {
            Some(colon) if stmt.kind == Statement::Declaration => Node::Declaration {
                name: parts[..colon].to_vec(),
                value: parts[colon + 1..].to_vec(),
            },
            _ => Node::Line(parts),
        };
        stack.last_mut().expect("the top stays").1.push(node);
    }
    close_to(&mut stack, 0);
    let mut top = stack.pop().expect("the top stays").1;
    top.extend(comments[next_comment..].iter().map(|&i| Node::Comment(i)));
    top
}

// ---- Nesting (from CSS) --------------------------------------------------------------------

/// A selector cut at its first part: `.a .b` is `.a` and `.b`, `.a:hover` is `.a` and
/// `&:hover`, `.a > .b` is `.a` and `> .b`. None for a list or a selector of one part.
fn split(selector: &str) -> Option<(String, String)> {
    if selector.contains(',') {
        return None;
    }
    let bytes = selector.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b' ' | b'>' | b'+' | b'~' if depth == 0 && i > 0 => {
                let head = selector[..i].trim();
                let tail = selector[i..].trim();
                return (!head.is_empty() && !tail.is_empty())
                    .then(|| (head.to_string(), tail.to_string()));
            }
            // A pseudo-class at the end of the first part.
            b':' if depth == 0 && i > 0 => {
                let head = selector[..i].trim();
                return Some((head.to_string(), format!("&{}", &selector[i..])));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The rules of CSS nested where they follow one another and share their first part.
fn nest(sheet: &Sheet, nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    for node in nodes {
        let node = match node {
            Node::Block { header, children } => {
                let children = nest(sheet, children);
                let selector = join(sheet, &header);
                let is_rule = !sheet.text[sheet.tokens[header[0]].range.clone()].starts_with('@');
                if !is_rule {
                    out.push(Node::Block { header, children });
                    continue;
                }
                Node::Nested { selector, children }
            }
            other => other,
        };
        let Node::Nested { selector, children } = node else {
            out.push(node);
            continue;
        };
        let parts = split(&selector);
        let previous = out.last_mut();
        match (parts, previous) {
            // The rule of the first part comes just before: the rest goes inside it.
            (
                Some((head, tail)),
                Some(Node::Nested {
                    selector: last,
                    children: inside,
                }),
            ) if *last == head => {
                let child = Node::Nested {
                    selector: tail,
                    children,
                };
                let merged = nest_one(inside, child);
                *inside = merged;
            }
            // The same selector again: its declarations join.
            (
                _,
                Some(Node::Nested {
                    selector: last,
                    children: inside,
                }),
            ) if *last == selector => {
                inside.extend(children);
            }
            (Some((head, tail)), _) => out.push(Node::Nested {
                selector: head,
                children: vec![Node::Nested {
                    selector: tail,
                    children,
                }],
            }),
            (None, _) => out.push(Node::Nested { selector, children }),
        }
    }
    out
}

/// Adds a nested rule to the end of a block, nesting it further when it can.
fn nest_one(inside: &mut Vec<Node>, child: Node) -> Vec<Node> {
    let mut nodes = std::mem::take(inside);
    let Node::Nested { selector, children } = child else {
        nodes.push(child);
        return nodes;
    };
    match (split(&selector), nodes.last_mut()) {
        (
            Some((head, tail)),
            Some(Node::Nested {
                selector: last,
                children: deeper,
            }),
        ) if *last == head && !head.starts_with(['>', '+', '~']) => {
            let merged = nest_one(
                deeper,
                Node::Nested {
                    selector: tail,
                    children,
                },
            );
            *deeper = merged;
        }
        (
            _,
            Some(Node::Nested {
                selector: last,
                children: same,
            }),
        ) if *last == selector => {
            same.extend(children);
        }
        _ => nodes.push(Node::Nested { selector, children }),
    }
    nodes
}

// ---- Printing ------------------------------------------------------------------------------

/// The text of a token in the target dialect: its variables renamed.
fn token_text(sheet: &Sheet, i: usize) -> String {
    let token = &sheet.tokens[i];
    let word = &sheet.text[token.range.clone()];
    match token.kind {
        Kind::Variable if word.starts_with('$') && sheet.to == Dialect::Less => {
            format!("@{}", &word[1..])
        }
        // A variable of Less: anything after `@` that is no at-rule of CSS (the parameters of a
        // mixin have no role of their own).
        Kind::AtRule
            if sheet.from == Dialect::Less
                && sheet.to != Dialect::Less
                && (sheet.roles[i] == Role::Variable || !is_css_at_rule(word)) =>
        {
            format!("${}", &word[1..])
        }
        _ => word.to_string(),
    }
}

fn is_css_at_rule(word: &str) -> bool {
    let name = word.trim_start_matches('@');
    super::data::AT_RULES
        .iter()
        .any(|(rule, dialects)| *rule == name && dialects.contains(['c', 'l']))
        || name.ends_with("keyframes")
}

/// Tokens on one line: a space where the text had one and after a comma, none inside
/// parentheses at their ends.
fn join(sheet: &Sheet, indices: &[usize]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < indices.len() {
        let index = indices[i];
        let token = &sheet.tokens[index];
        if i > 0 {
            let previous = &sheet.tokens[indices[i - 1]];
            let had = previous.range.end < token.range.start;
            let space = match (previous.kind, token.kind) {
                (Kind::Punct(b'('), _) | (_, Kind::Punct(b')' | b',' | b';')) => false,
                (Kind::Punct(b','), _) => true,
                (Kind::Interpolation, _) | (_, Kind::Interpolation) => had,
                _ => had,
            };
            if space {
                out.push(' ');
            }
        }
        // `#{$x}` of Sass is `@{x}` in Less; `!default` has no place there.
        if sheet.to == Dialect::Less {
            let word = &sheet.text[token.range.clone()];
            if token.kind == Kind::Interpolation
                && word.starts_with('#')
                && let (Some(&name), Some(&close)) = (indices.get(i + 1), indices.get(i + 2))
                && sheet.tokens[name].kind == Kind::Variable
                && sheet.tokens[close].kind == Kind::Interpolation
            {
                let variable = &sheet.text[sheet.tokens[name].range.clone()];
                out.push_str(&format!("@{{{}}}", variable.trim_start_matches('$')));
                i += 3;
                continue;
            }
            if token.kind == Kind::Bang && word == "!default" {
                i += 1;
                continue;
            }
        }
        out.push_str(&token_text(sheet, index));
        i += 1;
    }
    out.trim_end().to_string()
}

/// What a header or a line is, whatever the dialect it was written in.
enum Shape {
    /// A mixin declared: its name and its parameters (with their parentheses, or empty).
    Mixin(String, String),
    /// A mixin used.
    Include(String, String),
    /// `@extend` of a selector.
    Extend(String),
    Other(String),
}

fn shape(sheet: &Sheet, parts: &[usize], header: bool) -> Shape {
    let text = sheet.text;
    let word = |i: usize| &text[sheet.tokens[parts[i]].range.clone()];
    let rest = |from: usize| join(sheet, &parts[from.min(parts.len())..]);
    let first = word(0);
    // The name, and from where its parameters are: `name(…)`, or a name alone.
    let named = |at: usize| -> Option<(String, String)> {
        let name = parts.get(at).map(|_| word(at).to_string())?;
        Some((name, rest(at + 1)))
    };
    match first {
        "@mixin" if header => {
            if let Some((name, args)) = named(1) {
                return Shape::Mixin(name, args);
            }
        }
        "=" if header && sheet.from == Dialect::Sass => {
            if let Some((name, args)) = named(1) {
                return Shape::Mixin(name, args);
            }
        }
        "@include" if !header => {
            if let Some((name, args)) = named(1) {
                return Shape::Include(name, args);
            }
        }
        "+" if !header && sheet.from == Dialect::Sass => {
            if let Some((name, args)) = named(1) {
                return Shape::Include(name, args);
            }
        }
        "@extend" if !header => return Shape::Extend(rest(1)),
        // Less: `.m(@a) { … }` declares, `.m();` uses, `&:extend(.a);` extends.
        "." if sheet.from == Dialect::Less && parts.len() >= 2 => {
            let second = &sheet.tokens[parts[1]];
            let call = second.kind == Kind::Function;
            if header && call {
                return Shape::Mixin(word(1).to_string(), rest(2));
            }
            if !header {
                return Shape::Include(word(1).to_string(), rest(2));
            }
        }
        "&" if sheet.from == Dialect::Less
            && !header
            && parts.len() >= 4
            && word(2) == "extend" =>
        {
            // `& : extend ( … )`
            let inner = join(sheet, &parts[4..parts.len().saturating_sub(1)]);
            return Shape::Extend(inner);
        }
        _ => {}
    }
    Shape::Other(join(sheet, parts))
}

/// Parameters as the target writes them: `(…)` or nothing.
fn parameters(args: &str, target: Dialect) -> String {
    match (args.is_empty(), target) {
        (true, Dialect::Less) => "()".to_string(),
        (true, _) => String::new(),
        _ => args.to_string(),
    }
}

fn header_text(sheet: &Sheet, header: &[usize]) -> String {
    match shape(sheet, header, true) {
        Shape::Mixin(name, args) => {
            let args = parameters(&args, sheet.to);
            match sheet.to {
                Dialect::Less => format!(".{name}{args}"),
                Dialect::Sass => format!("={name}{args}"),
                _ => format!("@mixin {name}{args}"),
            }
        }
        Shape::Include(..) | Shape::Extend(_) => join(sheet, header),
        Shape::Other(text) => text,
    }
}

fn line_text(sheet: &Sheet, parts: &[usize]) -> String {
    match shape(sheet, parts, false) {
        Shape::Include(name, args) => {
            let args = parameters(&args, sheet.to);
            match sheet.to {
                Dialect::Less => format!(".{name}{args}"),
                Dialect::Sass => format!("+{name}{args}"),
                _ => format!("@include {name}{args}"),
            }
        }
        Shape::Extend(target) => match sheet.to {
            Dialect::Less => format!("&:extend({target})"),
            _ => format!("@extend {target}"),
        },
        Shape::Mixin(..) => join(sheet, parts),
        Shape::Other(text) => text,
    }
}

fn print(sheet: &Sheet, nodes: &[Node], depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let sass = sheet.to == Dialect::Sass;
    let end = if sass { "" } else { ";" };
    for (n, node) in nodes.iter().enumerate() {
        let is_block = |node: &Node| matches!(node, Node::Block { .. } | Node::Nested { .. });
        // The rules at the top are apart from one another.
        if depth == 0 && n > 0 && is_block(node) && is_block(&nodes[n - 1]) {
            out.push('\n');
        }
        match node {
            Node::Comment(i) => {
                let comment = &sheet.text[sheet.tokens[*i].range.clone()];
                if sass && comment.starts_with("/*") {
                    // Sass reads the lines of a block comment by their indentation: as
                    // line comments, nothing can go wrong.
                    let inner = comment.trim_start_matches("/*").trim_end_matches("*/");
                    for line in inner
                        .lines()
                        .map(|l| l.trim().trim_start_matches('*').trim())
                    {
                        if !line.is_empty() {
                            out.push_str(&format!("{indent}// {line}\n"));
                        }
                    }
                } else {
                    out.push_str(&format!("{indent}{comment}\n"));
                }
            }
            Node::Declaration { name, value } => {
                out.push_str(&format!(
                    "{indent}{}: {}{end}\n",
                    join(sheet, name),
                    join(sheet, value)
                ));
            }
            Node::Line(parts) => {
                out.push_str(&format!("{indent}{}{end}\n", line_text(sheet, parts)));
            }
            Node::Block { header, children } => {
                let head = header_text(sheet, header);
                print_block(sheet, &head, children, depth, out);
            }
            Node::Nested { selector, children } => {
                print_block(sheet, selector, children, depth, out);
            }
        }
    }
}

fn print_block(sheet: &Sheet, head: &str, children: &[Node], depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    if sheet.to == Dialect::Sass {
        out.push_str(&format!("{indent}{head}\n"));
        print(sheet, children, depth + 1, out);
    } else {
        out.push_str(&format!("{indent}{head} {{\n"));
        print(sheet, children, depth + 1, out);
        out.push_str(&format!("{indent}}}\n"));
    }
}

/// `text`, written in `from`, written again in `to`.
pub fn convert(text: &str, from: Dialect, to: Dialect) -> Result<String, String> {
    if to == Dialect::Css {
        return Err(tr!("css.cannot_convert_to_css").into());
    }
    let (tokens, unclosed) = lexer::tokens(text, from);
    if !unclosed.is_empty() {
        return Err(tr!("css.cannot_format_errors").into());
    }
    let roles = lexer::roles(text, &tokens, from);
    let sheet = Sheet {
        text,
        tokens,
        roles,
        from,
        to,
    };
    let mut nodes = parse(&sheet);
    if from == Dialect::Css {
        nodes = nest(&sheet, nodes);
    }
    let mut out = String::new();
    print(&sheet, &nodes, 0, &mut out);
    Ok(out)
}

/// The extension of a dialect.
pub fn extension(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Css => "css",
        Dialect::Scss => "scss",
        Dialect::Less => "less",
        Dialect::Sass => "sass",
    }
}
