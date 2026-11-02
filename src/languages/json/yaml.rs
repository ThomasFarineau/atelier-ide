//! YAML documents read as the same tree as JSON ones, with the place of each key and value, so
//! that a JSON Schema checks, completes and describes them too (GitHub workflows, Dependabot…).

use super::parse::{Kind, Member, Node, Spot, Step};
use serde_json::{Number, Value};
use std::{collections::HashMap, ops::Range};
use yaml_rust2::{
    Event, Yaml,
    parser::{MarkedEventReceiver, Parser},
    scanner::{Marker, TScalarStyle},
};

/// A syntax error: what is wrong, and where.
pub struct SyntaxError {
    pub message: String,
    pub range: Range<usize>,
}

enum Open {
    Mapping {
        start: usize,
        anchor: usize,
        members: Vec<Member>,
        key: Option<(String, Range<usize>)>,
    },
    Sequence {
        start: usize,
        anchor: usize,
        items: Vec<Node>,
    },
}

struct Builder<'a> {
    text: &'a str,
    /// The byte offset of each character: the parser counts characters.
    bytes: Vec<usize>,
    stack: Vec<Open>,
    documents: Vec<Node>,
    anchors: HashMap<usize, Node>,
}

impl Builder<'_> {
    fn offset(&self, mark: &Marker) -> usize {
        self.bytes
            .get(mark.index())
            .copied()
            .unwrap_or(self.text.len())
    }

    /// Where the text of a scalar ends. The parser only says where it starts.
    fn scalar_end(&self, start: usize, value: &str, style: TScalarStyle) -> usize {
        let rest = &self.text[start..];
        let line = rest.find('\n').map_or(rest.len(), |n| n);
        let end = match style {
            TScalarStyle::SingleQuoted => {
                let mut i = 1;
                let bytes = rest.as_bytes();
                loop {
                    match bytes.get(i) {
                        Some(b'\'') if bytes.get(i + 1) == Some(&b'\'') => i += 2,
                        Some(b'\'') | None => break i + 1,
                        Some(_) => i += 1,
                    }
                }
            }
            TScalarStyle::DoubleQuoted => {
                let mut i = 1;
                let bytes = rest.as_bytes();
                loop {
                    match bytes.get(i) {
                        Some(b'\\') => i += 2,
                        Some(b'"') | None => break i + 1,
                        Some(_) => i += 1,
                    }
                }
            }
            // The header of a block scalar.
            TScalarStyle::Literal | TScalarStyle::Folded => line,
            // A plain scalar on one line is its text.
            _ if rest[..line].starts_with(value) => value.len(),
            _ => line,
        };
        start + end.min(rest.len())
    }

    fn add(&mut self, node: Node) {
        match self.stack.last_mut() {
            None => self.documents.push(node),
            Some(Open::Sequence { items, .. }) => items.push(node),
            Some(Open::Mapping { members, key, .. }) => match key.take() {
                None => {
                    let name = match node.value() {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    *key = Some((name, node.range));
                }
                Some((key, key_range)) => members.push(Member {
                    key,
                    key_range,
                    value: node,
                }),
            },
        }
    }

    fn keep(&mut self, anchor: usize, node: &Node) {
        if anchor > 0 {
            self.anchors.insert(anchor, node.clone());
        }
    }
}

/// The value of a scalar: a plain one is read as YAML 1.2 does (numbers, booleans, null), a
/// quoted or block one is text.
fn scalar(value: String, style: TScalarStyle) -> Value {
    if style != TScalarStyle::Plain {
        return Value::String(value);
    }
    match Yaml::from_str(&value) {
        Yaml::Integer(i) => Value::from(i),
        Yaml::Real(r) => r
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map_or(Value::String(value), Value::Number),
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Null => Value::Null,
        _ => Value::String(value),
    }
}

impl MarkedEventReceiver for Builder<'_> {
    fn on_event(&mut self, event: Event, mark: Marker) {
        let at = self.offset(&mark);
        match event {
            Event::MappingStart(anchor, _) => self.stack.push(Open::Mapping {
                start: at,
                anchor,
                members: Vec::new(),
                key: None,
            }),
            Event::SequenceStart(anchor, _) => self.stack.push(Open::Sequence {
                start: at,
                anchor,
                items: Vec::new(),
            }),
            Event::MappingEnd => {
                if let Some(Open::Mapping {
                    start,
                    anchor,
                    mut members,
                    ..
                }) = self.stack.pop()
                {
                    // `<<: *defaults` brings the keys of another mapping; the ones written win.
                    let merged: Vec<Member> = members
                        .iter()
                        .filter(|m| m.key == "<<")
                        .flat_map(|m| match &m.value.kind {
                            Kind::Object(more) => more.clone(),
                            Kind::Array(maps) => maps
                                .iter()
                                .filter_map(|n| match &n.kind {
                                    Kind::Object(more) => Some(more.clone()),
                                    _ => None,
                                })
                                .flatten()
                                .collect(),
                            Kind::Scalar(_) => Vec::new(),
                        })
                        .collect();
                    members.retain(|m| m.key != "<<");
                    for member in merged {
                        if !members.iter().any(|m| m.key == member.key) {
                            members.push(member);
                        }
                    }
                    let node = Node {
                        kind: Kind::Object(members),
                        range: start..at.max(start),
                    };
                    self.keep(anchor, &node);
                    self.add(node);
                }
            }
            Event::SequenceEnd => {
                if let Some(Open::Sequence {
                    start,
                    anchor,
                    items,
                }) = self.stack.pop()
                {
                    let node = Node {
                        kind: Kind::Array(items),
                        range: start..at.max(start),
                    };
                    self.keep(anchor, &node);
                    self.add(node);
                }
            }
            Event::Scalar(value, style, anchor, _) => {
                let end = self.scalar_end(at, &value, style);
                let node = Node {
                    kind: Kind::Scalar(scalar(value, style)),
                    range: at..end,
                };
                self.keep(anchor, &node);
                self.add(node);
            }
            Event::Alias(anchor) => {
                let name = self.text[at..]
                    .find(|c: char| c.is_whitespace() || matches!(c, ',' | ']' | '}'))
                    .map_or(self.text.len(), |n| at + n);
                let node = match self.anchors.get(&anchor) {
                    Some(node) => Node {
                        kind: node.kind.clone(),
                        range: at..name,
                    },
                    None => Node {
                        kind: Kind::Scalar(Value::Null),
                        range: at..name,
                    },
                };
                self.add(node);
            }
            _ => {}
        }
    }
}

/// The documents of a YAML text.
pub fn parse(text: &str) -> Result<Vec<Node>, SyntaxError> {
    let mut builder = Builder {
        text,
        bytes: text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect(),
        stack: Vec::new(),
        documents: Vec::new(),
        anchors: HashMap::new(),
    };
    let mut parser = Parser::new_from_str(text);
    match parser.load(&mut builder, true) {
        Ok(()) => Ok(builder.documents),
        Err(error) => {
            let at = builder.offset(error.marker());
            let end = text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
            Err(SyntaxError {
                message: error.info().to_string(),
                range: at..end,
            })
        }
    }
}

/// The address of a schema named by a comment, as the YAML language server reads it:
/// `# yaml-language-server: $schema=./schema.json`.
pub fn schema_comment(text: &str) -> Option<String> {
    text.lines().take(20).find_map(|line| {
        let rest = line.trim_start().strip_prefix('#')?.trim_start();
        let rest = rest.strip_prefix("yaml-language-server:")?.trim_start();
        Some(rest.strip_prefix("$schema=")?.trim().to_string())
    })
}

const KEY: &str = "__atelier_key__";
const VALUE: &str = "__atelier_value__";

/// The way to the place of `marker` in the documents: to the member whose key is it, or to the
/// value that is it. With the other keys of its object.
fn find(node: &Node, marker: &str, path: &mut Vec<Step>) -> Option<Vec<String>> {
    match &node.kind {
        Kind::Object(members) => {
            if members.iter().any(|m| m.key == marker) {
                return Some(
                    members
                        .iter()
                        .filter(|m| m.key != marker)
                        .map(|m| m.key.clone())
                        .collect(),
                );
            }
            for m in members {
                path.push(Step::Key(m.key.clone()));
                if matches!(&m.value.kind, Kind::Scalar(Value::String(s)) if s == marker) {
                    return Some(Vec::new());
                }
                if let Some(present) = find(&m.value, marker, path) {
                    return Some(present);
                }
                path.pop();
            }
            None
        }
        Kind::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Step::Index(i));
                if matches!(&item.kind, Kind::Scalar(Value::String(s)) if s == marker) {
                    return Some(Vec::new());
                }
                if let Some(present) = find(item, marker, path) {
                    return Some(present);
                }
                path.pop();
            }
            None
        }
        Kind::Scalar(_) => None,
    }
}

/// What the caret at `offset` is on, and where the text it completes starts. The line of the
/// caret is written again with a marker in place of what is being typed, and the documents are
/// read to find it: the way to it is the one of the YAML itself.
pub fn spot(text: &str, offset: usize) -> Option<(Spot, usize)> {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let line_end = text[offset..].find('\n').map_or(text.len(), |n| offset + n);
    let before = &text[line_start..offset];
    let indent = before.len() - before.trim_start().len();
    let mut item = indent;
    let mut rest = &before[indent..];
    while let Some(after) = rest.strip_prefix("- ") {
        let trimmed = after.trim_start();
        item += rest.len() - trimmed.len();
        rest = trimmed;
    }
    let head = &before[..item];
    // `key: value` where the caret is in the value, else a key being typed.
    let colon = rest
        .find(": ")
        .map(|n| n + 1)
        .or(rest.ends_with(':').then_some(rest.len()));
    let (line, from, key, in_string) = match colon {
        Some(colon) => {
            let value = rest[colon..].trim_start();
            let value_start = offset - value.len();
            let quoted = value.starts_with(['"', '\'']);
            let from = if quoted { value_start + 1 } else { value_start };
            let key = &rest[..colon - 1];
            (
                format!("{head}{key}: {VALUE}"),
                from,
                false,
                quoted.then_some(from),
            )
        }
        None => {
            let word = rest.len()
                - rest
                    .trim_start_matches(|c: char| c.is_alphanumeric() || matches!(c, '_' | '-'))
                    .len();
            let from = if word == rest.len() {
                offset - rest.len()
            } else {
                offset
            };
            (format!("{head}{KEY}: 0"), from, true, None)
        }
    };
    let rewritten = |tail: &str| format!("{}{line}{tail}", &text[..line_start]);
    let documents = parse(&rewritten(&text[line_end..]))
        .or_else(|_| parse(&rewritten("")))
        .ok()?;
    let marker = if key { KEY } else { VALUE };
    documents.iter().find_map(|document| {
        let mut path = Vec::new();
        let present = find(document, marker, &mut path)?;
        Some((
            Spot {
                path,
                key,
                present,
                in_string,
            },
            from,
        ))
    })
}

/// A value as YAML writes it: plain when it reads back as the same text, else quoted.
pub fn plain(value: &str) -> String {
    match Yaml::from_str(value) {
        Yaml::String(_) if !value.is_empty() && !value.contains([':', '#', '\'', '"']) => {
            value.to_string()
        }
        _ => Value::String(value.to_string()).to_string(),
    }
}
