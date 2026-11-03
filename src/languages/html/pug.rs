//! Pug (formerly Jade): the colors of its lines, its tags and the code of other languages it
//! holds, read line by line as Pug reads them.
//!
//! A line is a tag (`a.button#main(href="/") text`), piped text (`| text`), a comment
//! (`//`, `//-`), code (`- let x = 1`, `= x`), a keyword (`if`, `each`, `mixin`, `include`…),
//! a mixin call (`+card(item)`) or a filter (`:markdown`). A tag that ends with `.` takes the
//! deeper lines as text: under `script.` they are JavaScript, under `style.` CSS, and they
//! go to those languages.

use super::embedded::{self, Region};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Text,
    Tag,
    Id,
    Class,
    AttrName,
    Keyword,
    Comment,
    /// A mixin or a block name.
    Name,
    String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PugAttr {
    pub name: String,
    pub name_range: Range<usize>,
    pub value: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PugTag {
    /// `div` for a tag written as `.class` or `#id` only.
    pub name: String,
    /// The name as written; None for `.class` and `#id`.
    pub name_range: Option<Range<usize>>,
    /// Where the tag starts on its line, and its indentation in columns.
    pub start: usize,
    pub indent: usize,
    pub id: Option<Range<usize>>,
    pub classes: Vec<Range<usize>>,
    pub attrs: Vec<PugAttr>,
    /// Inside its parentheses.
    pub attrs_range: Option<Range<usize>>,
    /// Nested on the line of another tag (`li: a`): not a line of its own.
    pub inline: bool,
}

#[derive(Default, Debug)]
pub struct Scan {
    pub tokens: Vec<(Range<usize>, Role)>,
    pub tags: Vec<PugTag>,
    pub regions: Vec<Region>,
    /// Problems: (range, is an error, message).
    pub problems: Vec<(Range<usize>, bool, String)>,
}

/// The words that start a line of their own.
pub const KEYWORDS: &[&str] = &[
    "doctype", "include", "extends", "block", "append", "prepend", "mixin", "if", "else", "unless",
    "case", "when", "default", "each", "for", "while", "yield",
];

/// What the deeper lines under a line are.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Comment,
    Code,
    Embedded(&'static str),
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':')
}

fn columns(indent: &str) -> usize {
    indent.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    scan: Scan,
}

impl Reader<'_> {
    fn token(&mut self, range: Range<usize>, role: Role) {
        if range.end > range.start {
            self.scan.tokens.push((range, role));
        }
    }

    /// Code that is only colored by JavaScript.
    fn code(&mut self, range: Range<usize>) {
        let range = trim(self.text, range);
        if range.end > range.start {
            self.scan.regions.push(Region {
                range,
                language: "js",
                analyzed: false,
                prefix: "",
                suffix: "",
            });
        }
    }

    /// Text with `#{…}` and `!{…}` interpolations, and `#[tag …]` tags, up to `end`.
    fn text(&mut self, mut at: usize, end: usize) {
        let mut start = at;
        while at < end {
            let b = self.bytes[at];
            if (b == b'#' || b == b'!') && self.bytes.get(at + 1) == Some(&b'{') {
                self.token(start..at, Role::Text);
                let close = matching(self.bytes, at + 1, end).unwrap_or(end);
                self.token(at..at + 2, Role::Keyword);
                self.code(at + 2..close);
                if close < end {
                    self.token(close..close + 1, Role::Keyword);
                }
                at = (close + 1).min(end);
                start = at;
                continue;
            }
            if b == b'#' && self.bytes.get(at + 1) == Some(&b'[') {
                self.token(start..at, Role::Text);
                let close = matching(self.bytes, at + 1, end).unwrap_or(end);
                self.token(at..at + 2, Role::Keyword);
                self.tag(at + 2, close, 0, true);
                if close < end {
                    self.token(close..close + 1, Role::Keyword);
                }
                at = (close + 1).min(end);
                start = at;
                continue;
            }
            at += 1;
        }
        self.token(start..end, Role::Text);
    }

    /// A tag from `at` to `end` (the end of its line). Returns what the deeper lines are.
    fn tag(&mut self, mut at: usize, end: usize, indent: usize, inline: bool) -> Option<Block> {
        let bytes = self.bytes;
        let mut tag = PugTag {
            name: "div".into(),
            name_range: None,
            start: at,
            indent,
            id: None,
            classes: Vec::new(),
            attrs: Vec::new(),
            attrs_range: None,
            inline,
        };
        if at < end && bytes[at].is_ascii_alphabetic() {
            let from = at;
            while at < end && is_ident(bytes[at]) {
                at += 1;
            }
            // `a:` is the tag `a` with a nested tag, not a name with a colon.
            while at > from && bytes[at - 1] == b':' {
                at -= 1;
            }
            tag.name = self.text[from..at].to_ascii_lowercase();
            tag.name_range = Some(from..at);
            self.token(from..at, Role::Tag);
        }
        loop {
            if at >= end {
                break;
            }
            match bytes[at] {
                b'.' if bytes.get(at + 1).is_some_and(|&b| is_ident(b) && b != b':') => {
                    let from = at;
                    at += 1;
                    while at < end && is_ident(bytes[at]) && bytes[at] != b':' {
                        at += 1;
                    }
                    tag.classes.push(from + 1..at);
                    self.token(from..at, Role::Class);
                }
                b'#' if bytes.get(at + 1).is_some_and(|&b| is_ident(b)) => {
                    let from = at;
                    at += 1;
                    while at < end && is_ident(bytes[at]) && bytes[at] != b':' {
                        at += 1;
                    }
                    tag.id = Some(from + 1..at);
                    self.token(from..at, Role::Id);
                }
                b'(' => {
                    let close = matching(bytes, at, self.text.len());
                    let stop = close.unwrap_or(self.text.len());
                    if close.is_none() {
                        self.scan.problems.push((
                            at..at + 1,
                            true,
                            tr!("html.pug_unclosed_attributes").into(),
                        ));
                    }
                    tag.attrs_range = Some(at + 1..stop);
                    self.attributes(&mut tag, at + 1, stop);
                    at = (stop + 1).min(self.text.len());
                    // The attributes may go over several lines.
                    if at > end {
                        let line_end = self.text[at..]
                            .find('\n')
                            .map_or(self.text.len(), |i| at + i);
                        return self.after_tag(tag, at, line_end);
                    }
                }
                b'&' if self.text[at..end].starts_with("&attributes(") => {
                    let open = at + "&attributes".len();
                    let close = matching(bytes, open, end).unwrap_or(end);
                    self.token(at..open, Role::Keyword);
                    self.code(open + 1..close);
                    at = (close + 1).min(end);
                }
                _ => break,
            }
        }
        let nothing = tag.name_range.is_none()
            && tag.id.is_none()
            && tag.classes.is_empty()
            && tag.attrs_range.is_none();
        if nothing {
            self.text(tag.start, end);
            return None;
        }
        self.after_tag(tag, at, end)
    }

    /// What follows the name, classes and attributes of a tag on its line.
    fn after_tag(&mut self, tag: PugTag, at: usize, end: usize) -> Option<Block> {
        let bytes = self.bytes;
        let name = tag.name.clone();
        let embedded = match name.as_str() {
            "script" => {
                let value = |n: &str| {
                    tag.attrs
                        .iter()
                        .find(|a| a.name == n)
                        .and_then(|a| a.value.clone())
                        .map(|r| unquote(&self.text[r]))
                };
                embedded::script_language(value("type"), value("lang"))
            }
            "style" => {
                let value = |n: &str| {
                    tag.attrs
                        .iter()
                        .find(|a| a.name == n)
                        .and_then(|a| a.value.clone())
                        .map(|r| unquote(&self.text[r]))
                };
                embedded::style_language(value("type"), value("lang"))
            }
            _ => None,
        };
        self.scan.tags.push(tag);
        if at >= end {
            return None;
        }
        match bytes[at] {
            // `tag.`: the deeper lines are its text.
            b'.' if self.text[at + 1..end].trim().is_empty() => {
                self.token(at..at + 1, Role::Keyword);
                Some(embedded.map_or(Block::Text, Block::Embedded))
            }
            b'/' => {
                self.token(at..at + 1, Role::Keyword);
                None
            }
            b':' if bytes.get(at + 1) == Some(&b' ') => {
                self.token(at..at + 1, Role::Keyword);
                let mut from = at + 1;
                while from < end && bytes[from] == b' ' {
                    from += 1;
                }
                self.tag(from, end, 0, true)
            }
            b'=' => {
                self.token(at..at + 1, Role::Keyword);
                self.code(at + 1..end);
                None
            }
            b'!' if bytes.get(at + 1) == Some(&b'=') => {
                self.token(at..at + 2, Role::Keyword);
                self.code(at + 2..end);
                None
            }
            b' ' | b'\t' => {
                // The text of `script` is its code.
                match embedded {
                    Some(language) if name == "script" => {
                        let range = trim(self.text, at..end);
                        self.scan.regions.push(Region {
                            range,
                            language,
                            analyzed: false,
                            prefix: "",
                            suffix: "",
                        });
                    }
                    _ => self.text(at + 1, end),
                }
                None
            }
            _ => {
                self.text(at, end);
                None
            }
        }
    }

    /// The attributes between `from` and `to` (inside the parentheses).
    fn attributes(&mut self, tag: &mut PugTag, from: usize, to: usize) {
        let bytes = self.bytes;
        let mut at = from;
        while at < to {
            while at < to && matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r' | b',') {
                at += 1;
            }
            if at >= to {
                break;
            }
            // The name, maybe quoted.
            let name_start = at;
            if matches!(bytes[at], b'"' | b'\'') {
                let quote = bytes[at];
                at += 1;
                while at < to && bytes[at] != quote {
                    at += 1;
                }
                at = (at + 1).min(to);
            } else {
                while at < to
                    && !matches!(
                        bytes[at],
                        b'=' | b'!' | b' ' | b'\t' | b'\n' | b'\r' | b',' | b')'
                    )
                {
                    at += 1;
                }
            }
            if at == name_start {
                at += 1;
                continue;
            }
            let name = self.text[name_start..at]
                .trim_matches(['"', '\''])
                .to_ascii_lowercase();
            self.token(name_start..at, Role::AttrName);
            let mut attr = PugAttr {
                name,
                name_range: name_start..at,
                value: None,
            };
            let mut k = at;
            while k < to && matches!(bytes[k], b' ' | b'\t') {
                k += 1;
            }
            let assign = if bytes.get(k) == Some(&b'=') {
                Some(1)
            } else if bytes[k..].starts_with(b"!=") {
                Some(2)
            } else {
                None
            };
            if let Some(len) = assign.filter(|_| k < to) {
                self.token(k..k + len, Role::Keyword);
                k += len;
                while k < to && matches!(bytes[k], b' ' | b'\t') {
                    k += 1;
                }
                let end = expression_end(bytes, k, to);
                attr.value = Some(k..end);
                let value = &self.text[k..end];
                let quoted = value.len() >= 2
                    && matches!(value.as_bytes()[0], b'"' | b'\'')
                    && value.as_bytes()[value.len() - 1] == value.as_bytes()[0]
                    && !value[1..value.len() - 1].contains(value.as_bytes()[0] as char);
                if quoted {
                    self.token(k..end, Role::String);
                } else {
                    self.code(k..end);
                }
                at = end;
            }
            tag.attrs.push(attr);
        }
    }

    /// A line that starts with a keyword. Returns what the deeper lines are, or None when the
    /// line is not a keyword line after all.
    fn keyword(&mut self, at: usize, end: usize) -> Option<Option<Block>> {
        let line = &self.text[at..end];
        let word_end = line
            .find(|c: char| !(c.is_ascii_alphanumeric()))
            .unwrap_or(line.len());
        let word = &line[..word_end];
        if !KEYWORDS.contains(&word) {
            return None;
        }
        // `if.` or `block(` would be a tag named so.
        if line[word_end..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
        {
            return None;
        }
        let rest = at + word_end..end;
        self.token(at..at + word_end, Role::Keyword);
        match word {
            "doctype" => self.token(rest, Role::Keyword),
            "include" | "extends" => {
                let range = trim(self.text, rest);
                let target = &self.text[range.clone()];
                // `include:markdown file.md` has a filter: the path is after it.
                self.token(
                    range,
                    if target.is_empty() {
                        Role::Text
                    } else {
                        Role::String
                    },
                );
            }
            "block" | "append" | "prepend" => {
                let range = trim(self.text, rest);
                let name_end = self.text[range.clone()]
                    .find(char::is_whitespace)
                    .map_or(range.end, |i| range.start + i);
                // `block append name`.
                let first = &self.text[range.start..name_end];
                if matches!(first, "append" | "prepend") {
                    self.token(range.start..name_end, Role::Keyword);
                    self.token(trim(self.text, name_end..range.end), Role::Name);
                } else {
                    self.token(range, Role::Name);
                }
            }
            "mixin" => {
                let range = trim(self.text, rest);
                let open = self.text[range.clone()]
                    .find('(')
                    .map_or(range.end, |i| range.start + i);
                self.token(range.start..open, Role::Name);
                if open < range.end {
                    self.code(open..range.end);
                }
            }
            "each" | "for" => {
                // `each item, index in items`: the names, then the expression.
                let line = &self.text[rest.clone()];
                match line.find(" in ") {
                    Some(i) => {
                        let keyword = rest.start + i + 1;
                        self.token(rest.start..keyword, Role::Text);
                        self.token(keyword..keyword + 2, Role::Keyword);
                        self.code(keyword + 2..rest.end);
                    }
                    None => self.code(rest),
                }
            }
            "else" => {
                let line = self.text[rest.clone()].trim_start();
                if let Some(condition) = line.strip_prefix("if") {
                    let start = rest.end - condition.len() - 2;
                    self.token(start..start + 2, Role::Keyword);
                    self.code(start + 2..rest.end);
                }
            }
            "when" => {
                // `when value: tag` puts a tag on the same line.
                let line = &self.text[rest.clone()];
                match line.find(": ") {
                    Some(i) => {
                        self.code(rest.start..rest.start + i);
                        self.token(rest.start + i..rest.start + i + 1, Role::Keyword);
                        let mut from = rest.start + i + 1;
                        while from < rest.end && self.bytes[from] == b' ' {
                            from += 1;
                        }
                        return Some(self.tag(from, rest.end, 0, true));
                    }
                    None => self.code(rest),
                }
            }
            "default" => {
                let line = &self.text[rest.clone()];
                if let Some(i) = line.find(": ") {
                    self.token(rest.start + i..rest.start + i + 1, Role::Keyword);
                    let mut from = rest.start + i + 1;
                    while from < rest.end && self.bytes[from] == b' ' {
                        from += 1;
                    }
                    return Some(self.tag(from, rest.end, 0, true));
                }
            }
            "yield" => {}
            _ => self.code(rest),
        }
        Some(None)
    }
}

/// The range without the spaces at its ends.
fn trim(text: &str, range: Range<usize>) -> Range<usize> {
    let part = &text[range.clone()];
    let start = range.start + (part.len() - part.trim_start().len());
    let end = range.end - (part.len() - part.trim_end().len());
    start..end.max(start)
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches(['"', '\'', '`'])
}

/// The bracket that closes the one at `open`, strings skipped; None when it is not closed
/// before `limit`.
fn matching(bytes: &[u8], open: usize, limit: usize) -> Option<usize> {
    let (o, c) = match bytes[open] {
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        _ => (b'{', b'}'),
    };
    let mut depth = 0usize;
    let mut i = open;
    while i < limit {
        match bytes[i] {
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < limit && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b if b == o => depth += 1,
            b if b == c => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Where the value of an attribute that starts at `from` ends: at a comma or a space outside
/// brackets and strings, unless the expression goes on (`a + b`).
fn expression_end(bytes: &[u8], from: usize, to: usize) -> usize {
    let mut depth = 0i32;
    let mut i = from;
    while i < to {
        match bytes[i] {
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < to && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth <= 0 => return i,
            b' ' | b'\t' | b'\n' | b'\r' if depth <= 0 => {
                let operator = |b: u8| b"+-*/%?:&|<>=!.".contains(&b);
                let previous = bytes[from..i]
                    .iter()
                    .rev()
                    .find(|b| !b.is_ascii_whitespace())
                    .copied();
                let next = bytes[i..to]
                    .iter()
                    .find(|b| !b.is_ascii_whitespace())
                    .copied();
                let goes_on = previous.is_some_and(operator) || next.is_some_and(operator);
                if !goes_on {
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    to.min(bytes.len())
}

/// Reads a Pug document.
pub fn scan(text: &str) -> Scan {
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        scan: Scan::default(),
    };
    // The block that the deeper lines belong to: its line's indentation, what it is, and
    // for a script or a style, where its code starts and ends so far.
    let mut block: Option<(usize, Block, Option<Range<usize>>)> = None;
    let mut indent_kind: Option<char> = None;
    let mut line_start = 0;
    // The line where a multi-line tag ended: its lines are already read.
    let mut skip_to = 0;
    for line in text.split('\n') {
        let start = line_start;
        let end = start + line.trim_end_matches('\r').len();
        line_start += line.len() + 1;
        if start < skip_to {
            continue;
        }
        let content = line.trim_start();
        let indent_text = &line[..line.len() - content.len()];
        let indent = columns(indent_text);
        let blank = content.trim().is_empty();
        if let Some((owner, kind, code)) = &mut block {
            if blank || indent > *owner {
                if !blank {
                    match kind {
                        Block::Comment => {
                            reader.token(start + indent_text.len()..end, Role::Comment)
                        }
                        Block::Text => reader.text(start + indent_text.len(), end),
                        Block::Code => reader.code(start..end),
                        Block::Embedded(_) => {
                            let range = code.get_or_insert(start..end);
                            range.end = end;
                        }
                    }
                }
                continue;
            }
            if let (Block::Embedded(language), Some(range)) = (*kind, code.take()) {
                reader.scan.regions.push(Region {
                    range,
                    language,
                    analyzed: true,
                    prefix: "",
                    suffix: "",
                });
            }
            block = None;
        }
        if blank {
            continue;
        }
        // Tabs and spaces mixed in the indentation of a file.
        if let Some(first) = indent_text.chars().next() {
            match indent_kind {
                None => indent_kind = Some(first),
                Some(kind) if indent_text.chars().any(|c| c != kind) => {
                    reader.scan.problems.push((
                        start..start + indent_text.len(),
                        false,
                        tr!("html.pug_mixed_indentation").into(),
                    ));
                }
                _ => {}
            }
        }
        let at = start + indent_text.len();
        let next: Option<Block> = if content.starts_with("//") {
            reader.token(at..end, Role::Comment);
            Some(Block::Comment)
        } else if let Some(rest) = content.strip_prefix('|') {
            reader.token(at..at + 1, Role::Keyword);
            reader.text(end - rest.len(), end);
            None
        } else if content.starts_with('<') {
            reader.token(at..end, Role::Text);
            None
        } else if let Some(code) = content.strip_prefix('-') {
            reader.token(at..at + 1, Role::Keyword);
            if code.trim().is_empty() {
                Some(Block::Code)
            } else {
                reader.code(at + 1..end);
                None
            }
        } else if content.starts_with("!=") {
            reader.token(at..at + 2, Role::Keyword);
            reader.code(at + 2..end);
            None
        } else if content.starts_with('=') {
            reader.token(at..at + 1, Role::Keyword);
            reader.code(at + 1..end);
            None
        } else if let Some(call) = content.strip_prefix('+') {
            // A mixin call, maybe with arguments and attributes.
            let name_end = call
                .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
                .map_or(end, |i| (at + 1 + i).min(end));
            reader.token(at..name_end, Role::Name);
            let mut rest = name_end;
            if reader.bytes.get(rest) == Some(&b'(') {
                let close = matching(reader.bytes, rest, end).unwrap_or(end);
                reader.code(rest + 1..close);
                rest = (close + 1).min(end);
            }
            // Its attributes and text, but it is not an element.
            let before = reader.scan.tags.len();
            reader.tag(rest, end, indent, true);
            reader.scan.tags.truncate(before);
            None
        } else if content.starts_with(':') {
            reader.token(at..end, Role::Keyword);
            Some(Block::Text)
        } else if let Some(next) = reader.keyword(at, end) {
            next
        } else {
            let before = reader.scan.tags.len();
            let next = reader.tag(at, end, indent, false);
            // A tag whose attributes went on over the next lines.
            if let Some(tag) = reader.scan.tags.get(before)
                && let Some(attrs) = &tag.attrs_range
                && attrs.end > end
            {
                skip_to = text[attrs.end..]
                    .find('\n')
                    .map_or(text.len(), |i| attrs.end + i + 1);
            }
            next
        };
        if let Some(kind) = next {
            block = Some((indent, kind, None));
        }
    }
    if let Some((_, Block::Embedded(language), Some(range))) = block {
        reader.scan.regions.push(Region {
            range,
            language,
            analyzed: true,
            prefix: "",
            suffix: "",
        });
    }
    reader.scan.tokens.sort_by_key(|(r, _)| r.start);
    reader.scan.regions.sort_by_key(|r| r.range.start);
    reader.scan
}
