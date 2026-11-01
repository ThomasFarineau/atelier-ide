//! Emmet abbreviations, expanded by Tab in HTML: `p#test` gives `<p id="test"></p>` with the
//! caret inside, `ul>li.item$*3` a list of three numbered items.
//!
//! Supported: names (`div`, implicit from the parent: `li` in `ul`, `td` in `tr`…), `#id`,
//! `.class`, `[attr=value]`, `{text}`, `>` child, `+` sibling, `^` up a level, `*3`
//! repeated, `$` the number of the copy (`$$` padded), `(…)` groups, and `!` for a page.

use super::data;

#[derive(Clone, Debug, Default)]
struct Node {
    /// Empty for an implicit name or a group.
    name: String,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
    text: Option<String>,
    repeat: usize,
    children: Vec<Node>,
    group: bool,
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

/// Where the caret goes once expanded: the first empty value or content.
const CARET: char = '\u{1}';

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Siblings up to the end, a `)`, or a `^` (returned: the levels still to climb).
    fn sequence(&mut self) -> Option<(Vec<Node>, usize)> {
        let mut siblings = Vec::new();
        loop {
            let mut item = self.item()?;
            match self.peek() {
                Some('+') => {
                    self.at += 1;
                    siblings.push(item);
                }
                Some('>') => {
                    self.at += 1;
                    let (children, climbs) = self.sequence()?;
                    item.children.extend(children);
                    siblings.push(item);
                    match climbs {
                        0 => return Some((siblings, 0)),
                        // Up to this level: the next one is a sibling here.
                        1 => {}
                        n => return Some((siblings, n - 1)),
                    }
                }
                Some('^') => {
                    let mut climbs = 0;
                    while self.eat('^') {
                        climbs += 1;
                    }
                    siblings.push(item);
                    return Some((siblings, climbs));
                }
                _ => {
                    siblings.push(item);
                    return Some((siblings, 0));
                }
            }
        }
    }

    fn word(&mut self, stop: &[char]) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if stop.contains(&c) || c.is_whitespace() {
                break;
            }
            out.push(c);
            self.at += 1;
        }
        out
    }

    fn item(&mut self) -> Option<Node> {
        let mut node = Node {
            repeat: 1,
            ..Node::default()
        };
        if self.eat('(') {
            let (children, _) = self.sequence()?;
            if !self.eat(')') {
                return None;
            }
            node.group = true;
            node.children = children;
        } else {
            node.name = self.word(&['#', '.', '[', '{', '>', '+', '^', '*', '(', ')']);
            if !node
                .name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | ':' | '!' | '$' | '_'))
            {
                return None;
            }
            loop {
                match self.peek() {
                    Some('#') => {
                        self.at += 1;
                        node.id = Some(self.word(&['#', '.', '[', '{', '>', '+', '^', '*', ')']));
                    }
                    Some('.') => {
                        self.at += 1;
                        let class = self.word(&['#', '.', '[', '{', '>', '+', '^', '*', ')']);
                        if class.is_empty() {
                            return None;
                        }
                        node.classes.push(class);
                    }
                    Some('[') => {
                        self.at += 1;
                        self.attributes(&mut node)?;
                    }
                    Some('{') => {
                        self.at += 1;
                        let mut depth = 1;
                        let mut text = String::new();
                        loop {
                            let c = self.peek()?;
                            self.at += 1;
                            match c {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            text.push(c);
                        }
                        node.text = Some(text);
                    }
                    _ => break,
                }
            }
            if node.name.is_empty()
                && node.id.is_none()
                && node.classes.is_empty()
                && node.attrs.is_empty()
                && node.text.is_none()
            {
                return None;
            }
        }
        if self.eat('*') {
            let digits = self.word(&['#', '.', '[', '{', '>', '+', '^', '*', '(', ')']);
            node.repeat = digits.parse().ok().filter(|n| (1..=1000).contains(n))?;
        }
        Some(node)
    }

    /// `[a=b c="d e" f]`, after its `[`.
    fn attributes(&mut self, node: &mut Node) -> Option<()> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.at += 1;
            }
            if self.eat(']') {
                return Some(());
            }
            let name = self.word(&['=', ']']);
            if name.is_empty() {
                return None;
            }
            let value = if self.eat('=') {
                Some(match self.peek()? {
                    quote @ ('"' | '\'') => {
                        self.at += 1;
                        let mut value = String::new();
                        loop {
                            let c = self.peek()?;
                            self.at += 1;
                            if c == quote {
                                break;
                            }
                            value.push(c);
                        }
                        value
                    }
                    _ => self.word(&[']']),
                })
            } else {
                None
            };
            node.attrs.push((name, value));
        }
    }
}

/// The name an element gets when only its id or classes are written, from its parent.
fn implicit_name(parent: &str) -> &'static str {
    match parent {
        "ul" | "ol" | "menu" => "li",
        "table" | "tbody" | "thead" | "tfoot" => "tr",
        "tr" => "td",
        "select" | "optgroup" | "datalist" => "option",
        "audio" | "video" | "picture" => "source",
        "map" => "area",
        "em" | "strong" | "span" | "b" | "i" | "a" | "label" | "small" | "code" | "p" => "span",
        _ => "div",
    }
}

/// The attributes an element is written with, as Emmet does (`a` gets an `href`).
fn default_attributes(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "a" => &[("href", "")],
        "img" => &[("src", ""), ("alt", "")],
        "input" => &[("type", "text")],
        "label" => &[("for", "")],
        "form" => &[("action", "")],
        "link" => &[("rel", "stylesheet"), ("href", "")],
        "iframe" | "embed" | "audio" | "video" => &[("src", "")],
        "area" => &[("shape", ""), ("coords", ""), ("href", ""), ("alt", "")],
        "meta" => &[("name", ""), ("content", "")],
        "time" => &[("datetime", "")],
        "object" => &[("data", ""), ("type", "")],
        "abbr" => &[("title", "")],
        _ => &[],
    }
}

/// `$` replaced by the number of the copy, `$$$` padded with zeros.
fn numbered(text: &str, number: usize) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut width = 1;
            while chars.peek() == Some(&'$') {
                chars.next();
                width += 1;
            }
            out.push_str(&format!("{number:0width$}"));
        } else {
            out.push(c);
        }
    }
    out
}

fn is_inline(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr"
            | "b"
            | "br"
            | "button"
            | "cite"
            | "code"
            | "em"
            | "i"
            | "img"
            | "input"
            | "kbd"
            | "label"
            | "mark"
            | "q"
            | "s"
            | "select"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "textarea"
            | "time"
            | "u"
    )
}

/// An element made concrete: its name, the copies of the groups undone, `$` numbered.
#[derive(Debug)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    children: Vec<Element>,
}

fn resolve(nodes: &[Node], parent: &str, number: Option<usize>, out: &mut Vec<Element>) {
    for node in nodes {
        for copy in 1..=node.repeat {
            let n = if node.repeat > 1 { Some(copy) } else { number };
            let fill = |s: &str| match n {
                Some(n) => numbered(s, n),
                None => s.to_string(),
            };
            if node.group {
                resolve(&node.children, parent, n, out);
                continue;
            }
            let name = if node.name.is_empty() {
                implicit_name(parent).to_string()
            } else {
                fill(&node.name)
            };
            let mut attrs: Vec<(String, String)> = Vec::new();
            if let Some(id) = &node.id {
                attrs.push(("id".into(), fill(id)));
            }
            if !node.classes.is_empty() {
                let classes: Vec<String> = node.classes.iter().map(|c| fill(c)).collect();
                attrs.push(("class".into(), classes.join(" ")));
            }
            let written: Vec<(String, String)> = node
                .attrs
                .iter()
                .map(|(k, v)| (k.clone(), v.as_deref().map(fill).unwrap_or_default()))
                .collect();
            // The usual attributes of the element, unless written.
            for (key, value) in default_attributes(&name) {
                if !written.iter().any(|(k, _)| k == key) && !attrs.iter().any(|(k, _)| k == key) {
                    attrs.push((key.to_string(), value.to_string()));
                }
            }
            attrs.extend(written);
            let mut children = Vec::new();
            resolve(&node.children, &name, n, &mut children);
            out.push(Element {
                name,
                attrs,
                text: node.text.as_deref().map(fill),
                children,
            });
        }
    }
}

fn render(elements: &[Element], depth: usize, indent: &str, out: &mut String) {
    let block = elements.iter().any(|e| !is_inline(&e.name)) || elements.len() > 1;
    for (i, element) in elements.iter().enumerate() {
        if block && (i > 0 || depth > 0) {
            out.push('\n');
            out.push_str(&indent.repeat(depth));
        }
        out.push('<');
        out.push_str(&element.name);
        for (key, value) in &element.attrs {
            if data::BOOLEAN_ATTRIBUTES.contains(&key.as_str()) && value.is_empty() {
                out.push(' ');
                out.push_str(key);
                continue;
            }
            out.push_str(&format!(" {key}=\""));
            if value.is_empty() {
                out.push(CARET);
            }
            out.push_str(value);
            out.push('"');
        }
        out.push('>');
        if data::is_void(&element.name) {
            continue;
        }
        if let Some(text) = &element.text {
            out.push_str(text);
        }
        let children_block = element.children.iter().any(|e| !is_inline(&e.name))
            || element.children.len() > 1
                && element.children.iter().any(|e| !e.children.is_empty());
        if element.children.is_empty() {
            if element.text.is_none() {
                out.push(CARET);
            }
        } else if children_block {
            render(&element.children, depth + 1, indent, out);
            out.push('\n');
            out.push_str(&indent.repeat(depth));
        } else {
            let mut inner = String::new();
            render(&element.children, 0, indent, &mut inner);
            out.push_str(&inner);
        }
        out.push_str(&format!("</{}>", element.name));
    }
}

/// The page that `!` gives.
fn page(indent: &str) -> String {
    let i = indent;
    format!(
        "<!doctype html>\n<html lang=\"{CARET}en\">\n<head>\n{i}<meta charset=\"UTF-8\">\n{i}<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n{i}<title>Document</title>\n</head>\n<body>\n{i}{CARET}\n</body>\n</html>"
    )
}

/// The HTML of an abbreviation, with the place of the caret in it; None when it is not one.
pub fn expand(abbreviation: &str, indent: &str) -> Option<(String, usize)> {
    let raw = if abbreviation == "!" {
        page(indent)
    } else {
        let mut parser = Parser {
            chars: abbreviation.chars().collect(),
            at: 0,
        };
        let (nodes, _) = parser.sequence()?;
        if parser.at != parser.chars.len() {
            return None;
        }
        let mut elements = Vec::new();
        resolve(&nodes, "", None, &mut elements);
        let mut out = String::new();
        render(&elements, 0, indent, &mut out);
        out
    };
    let caret = raw.find(CARET).unwrap_or(raw.len());
    Some((raw.replace(CARET, ""), caret))
}

/// The abbreviation that ends at `offset` on its line: from after the last space, or the last
/// tag, before it (brackets and braces may hold spaces).
fn abbreviation_start(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut depth = 0i32;
    let mut i = bytes.len();
    while i > 0 {
        let b = bytes[i - 1];
        match b {
            b']' | b'}' => depth += 1,
            b'[' | b'{' => depth -= 1,
            b' ' | b'\t' if depth <= 0 => break,
            // The end of a tag written before it: `<div>ul>li`.
            b'>' if depth <= 0 && line[..i - 1].rfind('<') > line[..i - 1].rfind('>') => break,
            _ => {}
        }
        i -= 1;
    }
    i
}

/// Whether a word alone is worth expanding: a known element, or a custom one.
fn expands_alone(word: &str) -> bool {
    data::is_element(word)
        || (word.contains('-') && word.chars().next().is_some_and(char::is_alphabetic))
}

/// The abbreviation that ends at `offset` in an HTML document and its expansion: the range it
/// takes, the HTML (indented from its line), and the caret's place in it.
pub fn expand_at(
    text: &str,
    offset: usize,
    indent: &str,
) -> Option<(std::ops::Range<usize>, String, usize)> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    // In the markup only: not in a tag, a comment or the code of a script.
    let scan = super::lexer::scan(text);
    if super::lexer::tag_at(&scan.tags, offset).is_some() {
        return None;
    }
    if let Some(token) = super::lexer::token_at(&scan.tokens, offset)
        && token.range.start < offset
        && matches!(
            token.kind,
            super::lexer::Kind::Comment
                | super::lexer::Kind::RawText
                | super::lexer::Kind::Declaration
        )
    {
        return None;
    }
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..offset];
    let start = abbreviation_start(line);
    let abbreviation = &line[start..];
    let first = abbreviation.chars().next()?;
    if !(first.is_alphabetic() || matches!(first, '.' | '#' | '[' | '(' | '!' | '{')) {
        return None;
    }
    let operators = abbreviation.contains(['#', '.', '>', '+', '^', '*', '[', '{', '(', '!']);
    if !operators && !expands_alone(abbreviation) {
        return None;
    }
    let (html, caret) = expand(abbreviation, indent)?;
    // The lines after the first take the indentation of the line.
    let base: String = text[line_start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let shown = html.replace('\n', &format!("\n{base}"));
    // The caret moves by the indentation added before it.
    let caret = caret + html[..caret].matches('\n').count() * base.len();
    let start = line_start + start;
    Some((start..offset, shown, caret))
}
