//! A JSON document with the place of each value and key, to put a problem where it is and to
//! know what the caret is on. Comments and trailing commas, which many JSON files of tools have
//! (`tsconfig.json`, the settings of editors), are read as spaces.

use serde_json::Value;
use std::ops::Range;

#[derive(Clone)]
pub enum Kind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    /// Strings, numbers, booleans and null.
    Scalar(Value),
}

#[derive(Clone)]
pub struct Node {
    pub kind: Kind,
    pub range: Range<usize>,
}

#[derive(Clone)]
pub struct Member {
    pub key: String,
    /// The key with its quotes.
    pub key_range: Range<usize>,
    pub value: Node,
}

impl Node {
    pub fn value(&self) -> Value {
        match &self.kind {
            Kind::Object(members) => Value::Object(
                members
                    .iter()
                    .map(|m| (m.key.clone(), m.value.value()))
                    .collect(),
            ),
            Kind::Array(items) => Value::Array(items.iter().map(Node::value).collect()),
            Kind::Scalar(value) => value.clone(),
        }
    }

    pub fn member(&self, key: &str) -> Option<&Member> {
        match &self.kind {
            Kind::Object(members) => members.iter().find(|m| m.key == key),
            _ => None,
        }
    }
}

/// The text with its comments, and the commas before a closing bracket, turned into spaces: the
/// places of the rest do not move.
pub fn clean(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let blank = |out: &mut Vec<u8>, range: Range<usize>| {
        for b in &mut out[range] {
            if *b != b'\n' && *b != b'\r' {
                *b = b' ';
            }
        }
    };
    let mut i = 0;
    // The last comma outside strings, to blank when a bracket closes after it.
    let mut comma: Option<usize> = None;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                comma = None;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' && bytes[i] != b'\n' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                blank(&mut out, i..end);
                i = end;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |n| i + 2 + n + 2);
                blank(&mut out, i..end);
                i = end;
                continue;
            }
            b',' => comma = Some(i),
            b'}' | b']' => {
                if let Some(at) = comma.take() {
                    out[at] = b' ';
                }
            }
            b if b.is_ascii_whitespace() => {}
            _ => comma = None,
        }
        i += 1;
    }
    // Only ASCII bytes were replaced, by ASCII spaces: still UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// The document, when `text` (cleaned) is valid JSON.
pub fn parse(text: &str) -> Option<Node> {
    let mut parser = Parser { text, at: 0 };
    let node = parser.value()?;
    parser.space();
    (parser.at == text.len()).then_some(node)
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        let rest = &self.text[self.at..];
        self.at += rest.len() - rest.trim_start().len();
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn value(&mut self) -> Option<Node> {
        self.space();
        let start = self.at;
        let kind = match self.peek()? {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                } else {
                    loop {
                        self.space();
                        let key_start = self.at;
                        let key = self.string()?;
                        let key_range = key_start..self.at;
                        self.space();
                        (self.peek()? == b':').then_some(())?;
                        self.at += 1;
                        let value = self.value()?;
                        members.push(Member {
                            key,
                            key_range,
                            value,
                        });
                        self.space();
                        match self.peek()? {
                            b',' => self.at += 1,
                            b'}' => {
                                self.at += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Kind::Object(members)
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                } else {
                    loop {
                        items.push(self.value()?);
                        self.space();
                        match self.peek()? {
                            b',' => self.at += 1,
                            b']' => {
                                self.at += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Kind::Array(items)
            }
            b'"' => Kind::Scalar(Value::String(self.string()?)),
            _ => {
                let rest = &self.text[self.at..];
                let len = rest
                    .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']'))
                    .unwrap_or(rest.len());
                let value: Value = serde_json::from_str(&rest[..len]).ok()?;
                self.at += len;
                Kind::Scalar(value)
            }
        };
        Some(Node {
            kind,
            range: start..self.at,
        })
    }

    fn string(&mut self) -> Option<String> {
        let rest = &self.text[self.at..];
        let bytes = rest.as_bytes();
        (bytes.first() == Some(&b'"')).then_some(())?;
        let mut i = 1;
        while i < bytes.len() && bytes[i] != b'"' {
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        let quoted = rest.get(..=i)?;
        let value = serde_json::from_str(quoted).ok()?;
        self.at += i + 1;
        Some(value)
    }
}

/// One step of the way to a value: a key of an object or an index of an array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

/// What the caret is on, read from the start of the text: the text may be unfinished.
#[derive(Debug, PartialEq, Eq)]
pub struct Spot {
    /// Where the object or the value is.
    pub path: Vec<Step>,
    /// A key is expected (else a value).
    pub key: bool,
    /// The keys already written in the object, before the caret.
    pub present: Vec<String>,
    /// The start of the text of the string the caret is in, after its quote.
    pub in_string: Option<usize>,
}

enum Frame {
    Object {
        key: Option<String>,
        expects_key: bool,
        present: Vec<String>,
    },
    Array {
        index: usize,
    },
}

/// What the caret at `offset` of the cleaned `text` is on.
pub fn spot(text: &str, offset: usize) -> Spot {
    let bytes = text.as_bytes();
    let offset = offset.min(text.len());
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;
    let mut in_string = None;
    while i < offset {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'"' && bytes[end] != b'\n' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                // The caret is before its closing quote, or on its line when it is not closed.
                if offset <= end {
                    in_string = Some(start);
                    break;
                }
                if end < bytes.len()
                    && bytes[end] == b'"'
                    && let Some(Frame::Object {
                        key,
                        expects_key: true,
                        ..
                    }) = stack.last_mut()
                {
                    *key = serde_json::from_str(&text[i..=end]).ok();
                }
                i = end + 1;
                continue;
            }
            b'{' => stack.push(Frame::Object {
                key: None,
                expects_key: true,
                present: Vec::new(),
            }),
            b'[' => stack.push(Frame::Array { index: 0 }),
            b'}' | b']' => {
                stack.pop();
            }
            b':' => {
                if let Some(Frame::Object {
                    key,
                    expects_key,
                    present,
                }) = stack.last_mut()
                {
                    *expects_key = false;
                    if let Some(key) = key {
                        present.push(key.clone());
                    }
                }
            }
            b',' => match stack.last_mut() {
                Some(Frame::Object {
                    key, expects_key, ..
                }) => {
                    *key = None;
                    *expects_key = true;
                }
                Some(Frame::Array { index }) => *index += 1,
                None => {}
            },
            _ => {}
        }
        i += 1;
    }
    let mut path = Vec::new();
    let mut key = false;
    let mut present = Vec::new();
    let last = stack.len().saturating_sub(1);
    for (depth, frame) in stack.into_iter().enumerate() {
        match frame {
            Frame::Object {
                key: name,
                expects_key,
                present: written,
            } => {
                if depth == last && expects_key {
                    key = true;
                    present = written;
                } else {
                    path.push(Step::Key(name.unwrap_or_default()));
                }
            }
            Frame::Array { index } => path.push(Step::Index(index)),
        }
    }
    Spot {
        path,
        key,
        present,
        in_string,
    }
}

/// The way to the member whose key is at `offset`, and that member.
pub fn key_at(node: &Node, offset: usize) -> Option<(Vec<Step>, &Member)> {
    match &node.kind {
        Kind::Object(members) => members.iter().find_map(|m| {
            if m.key_range.contains(&offset) {
                return Some((vec![Step::Key(m.key.clone())], m));
            }
            let (mut path, member) = key_at(&m.value, offset)?;
            path.insert(0, Step::Key(m.key.clone()));
            Some((path, member))
        }),
        Kind::Array(items) => items.iter().enumerate().find_map(|(i, item)| {
            let (mut path, member) = key_at(item, offset)?;
            path.insert(0, Step::Index(i));
            Some((path, member))
        }),
        Kind::Scalar(_) => None,
    }
}
