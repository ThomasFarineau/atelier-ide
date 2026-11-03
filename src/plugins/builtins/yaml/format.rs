//! Reformat Code of YAML and TOML files. Only the layout changes: the indentation, the spaces
//! after a key and the blank lines. The comments, the quotes and the order of the file stay. The
//! data is read before and after, and a reformat that would change it is refused.

use serde_json::{Map, Value};
use yaml_rust2::{Yaml, YamlLoader};

/// Spaces per level of nesting.
const INDENT: usize = 2;
/// Blank lines kept in a row, as IntelliJ keeps them.
const BLANK_LINES: usize = 2;

pub fn yaml(text: &str) -> Result<String, String> {
    let before = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    let formatted = layout_yaml(text);
    match YamlLoader::load_from_str(&formatted) {
        Ok(after) if after == before => Ok(formatted),
        _ => Err(tr!("yaml.format_changes_data").into()),
    }
}

pub fn toml(text: &str) -> Result<String, String> {
    let before = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    let formatted = layout_toml(text);
    match formatted.parse::<toml_edit::DocumentMut>() {
        Ok(after) if toml_data(before.as_item()) == toml_data(after.as_item()) => Ok(formatted),
        _ => Err(tr!("yaml.format_changes_data").into()),
    }
}

/// Joins the lines with the line break of the file, the last one kept or not as it was.
fn join(lines: Vec<String>, text: &str) -> String {
    let mut lines = lines;
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = lines.join(newline);
    if text.ends_with('\n') && !out.is_empty() {
        out.push_str(newline);
    }
    out
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The open blocks of a YAML document: the column each starts at in the file, and the one it
/// gets.
#[derive(Default)]
struct Blocks(Vec<(usize, usize)>);

impl Blocks {
    /// The new column of a line at `column`, which opens a block when it is deeper.
    fn place(&mut self, column: usize) -> usize {
        while self.0.last().is_some_and(|(old, _)| *old > column) {
            self.0.pop();
        }
        match self.0.last() {
            Some((old, new)) if *old == column => *new,
            last => {
                let new = last.map_or(0, |(_, new)| new + INDENT);
                self.0.push((column, new));
                new
            }
        }
    }

    /// The new column of a comment at `column`: the one of the block it lines up with, without
    /// opening or closing any.
    fn comment(&self, column: usize) -> usize {
        match self.0.iter().rev().find(|(old, _)| *old <= column) {
            Some((old, new)) if *old == column => *new,
            Some((_, new)) => new + INDENT,
            None => 0,
        }
    }
}

/// A block scalar (`|` or `>`) being copied: its lines keep their indentation relative to the
/// first one.
struct Scalar {
    /// The column of the node that holds it: its lines are deeper.
    parent: usize,
    /// Where its lines start in the file (known at the first one) and where they go.
    from: Option<usize>,
    to: usize,
}

fn layout_yaml(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<String> = Vec::new();
    let mut blocks = Blocks::default();
    let mut scalar: Option<Scalar> = None;
    let mut blanks = 0;
    for line in text.lines() {
        let column = indentation(line);
        let content = line.trim();
        if let Some(s) = &mut scalar {
            if content.is_empty() && line.len() <= s.from.unwrap_or(usize::MAX) {
                out.push(String::new());
                continue;
            }
            if column > s.parent {
                let from = *s.from.get_or_insert(column);
                let kept = column.min(from);
                let shifted = (s.to + kept).saturating_sub(from);
                out.push(format!("{}{}", " ".repeat(shifted), &line[kept..]));
                continue;
            }
            scalar = None;
        }
        if content.is_empty() {
            blanks += 1;
            if blanks <= BLANK_LINES {
                out.push(String::new());
            }
            continue;
        }
        blanks = 0;
        if column == 0
            && (content.starts_with("---")
                || content.starts_with("...")
                || content.starts_with('%'))
        {
            blocks = Blocks::default();
            out.push(content.to_string());
            continue;
        }
        if content.starts_with('#') {
            out.push(format!("{}{content}", " ".repeat(blocks.comment(column))));
            continue;
        }
        let new = blocks.place(column);
        // `-   item` becomes `- item`; what follows a dash is a block of its own, for the lines
        // under it that line up with it.
        let mut rest = content;
        // The columns of the text after the dashes, and of the last dash: in the file, and new.
        let mut at = (column, new);
        let mut dash = at;
        let mut dashes = String::new();
        while let Some(after) = rest.strip_prefix('-')
            && after.starts_with([' ', '\t'])
        {
            let item = after.trim_start();
            if item.is_empty() || item.starts_with('#') {
                break;
            }
            dash = at;
            at = (at.0 + rest.len() - item.len(), at.1 + INDENT);
            blocks.place(at.0);
            dashes.push_str("- ");
            rest = item;
        }
        let rest = key_spacing(rest);
        if block_scalar(&rest) {
            // The lines of a block scalar are deeper than the key, or the dash, that holds it.
            let parent = if rest.starts_with(['|', '>']) && !dashes.is_empty() {
                dash
            } else {
                at
            };
            scalar = Some(Scalar {
                parent: parent.0,
                from: None,
                to: parent.1 + INDENT,
            });
        }
        out.push(format!("{}{dashes}{rest}", " ".repeat(new)));
    }
    join(out, text)
}

/// `key:   value` becomes `key: value`: the spaces after the colon of a mapping key, outside
/// quotes and brackets.
fn key_spacing(line: &str) -> String {
    if line.starts_with(['?', '{', '[', '"', '\'']) && !quoted_key(line) {
        return line.to_string();
    }
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' if i == 0 => quote = Some(b),
                b'#' if i > 0 && bytes[i - 1] == b' ' => return line.to_string(),
                b'{' | b'[' => return line.to_string(),
                b':' if bytes.get(i + 1).is_some_and(|c| *c == b' ' || *c == b'\t') => {
                    let value = line[i + 1..].trim_start();
                    return if value.is_empty() {
                        line[..=i].to_string()
                    } else {
                        format!("{} {value}", &line[..=i])
                    };
                }
                _ => {}
            },
        }
    }
    line.to_string()
}

/// A line that starts with a quoted key (`"a b": 1`).
fn quoted_key(line: &str) -> bool {
    let Some(q) = line.chars().next().filter(|c| *c == '"' || *c == '\'') else {
        return false;
    };
    line[1..]
        .find(q)
        .is_some_and(|end| line[end + 2..].trim_start().starts_with(':'))
}

/// A line that ends with the header of a block scalar: `key: |`, `- >-`, `key: |2 # note`.
fn block_scalar(line: &str) -> bool {
    let code = match line.find(" #") {
        Some(i) => &line[..i],
        None => line,
    }
    .trim_end();
    let Some(start) = code.rfind(['|', '>']) else {
        return false;
    };
    let header = &code[start + 1..];
    // An anchor or a tag may come before it: `key: &text |`.
    let mut before = code[..start].trim_end();
    while let Some(token) = before.rsplit(' ').next()
        && token.starts_with(['&', '!'])
    {
        before = before[..before.len() - token.len()].trim_end();
    }
    header
        .chars()
        .all(|c| c == '-' || c == '+' || c.is_ascii_digit())
        && (before.is_empty() || before.ends_with(':'))
}

/// Where a TOML line leaves off: in a multi-line string, and how deep in arrays.
#[derive(Default)]
struct TomlState {
    string: Option<&'static str>,
    depth: usize,
}

/// Reads the code of a line to know whether it opens or closes a multi-line string
/// or an array.
fn scan_toml(line: &str, state: &mut TomlState) {
    let mut i = 0;
    let bytes = line.as_bytes();
    while i < bytes.len() {
        if let Some(close) = state.string {
            match line[i..].find(close) {
                Some(end) => {
                    i += end + close.len();
                    // A quote right after the closing (`""""`) belongs to the string.
                    while i < bytes.len() && bytes[i] == close.as_bytes()[0] {
                        i += 1;
                    }
                    state.string = None;
                }
                None => return,
            }
            continue;
        }
        let rest = &line[i..];
        if rest.starts_with("\"\"\"") || rest.starts_with("'''") {
            state.string = Some(if rest.starts_with('"') {
                "\"\"\""
            } else {
                "'''"
            });
            i += 3;
            continue;
        }
        match bytes[i] {
            b'#' => return,
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
            }
            b'[' | b'{' => state.depth += 1,
            b']' | b'}' => state.depth = state.depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
}

/// The end of the key of a TOML line: its `=`, outside quotes.
fn toml_equals(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '=' => return Some(i),
                '#' => return None,
                _ => {}
            },
        }
    }
    None
}

/// `[ table ]` becomes `[table]`, `[[ array ]]` becomes `[[array]]`.
fn toml_header(line: &str) -> String {
    let (open, close) = if line.starts_with("[[") {
        ("[[", "]]")
    } else {
        ("[", "]")
    };
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices().skip(open.len()) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if line[i..].starts_with(close) => {
                let name = line[open.len()..i].trim();
                let rest = line[i + close.len()..].trim_start();
                return match rest.is_empty() {
                    true => format!("{open}{name}{close}"),
                    false => format!("{open}{name}{close} {rest}"),
                };
            }
            None => {}
        }
    }
    line.to_string()
}

fn layout_toml(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<String> = Vec::new();
    let mut state = TomlState::default();
    let mut blanks = 0;
    for line in text.lines() {
        if state.string.is_some() {
            // The text of a multi-line string, as it is.
            out.push(line.to_string());
            scan_toml(line, &mut state);
            continue;
        }
        let content = line.trim();
        if content.is_empty() {
            blanks += 1;
            if blanks <= BLANK_LINES {
                out.push(String::new());
            }
            continue;
        }
        blanks = 0;
        let depth = state.depth;
        let formatted = if depth > 0 {
            // An item of an array that spans lines; its closing bracket goes back one level.
            let level = depth - usize::from(content.starts_with([']', '}']));
            format!("{}{content}", " ".repeat(level * INDENT))
        } else if content.starts_with('[') {
            toml_header(content)
        } else if let Some(at) = toml_equals(content).filter(|_| !content.starts_with('#')) {
            format!(
                "{} = {}",
                content[..at].trim_end(),
                content[at + 1..].trim_start()
            )
        } else {
            content.to_string()
        };
        // A line that opens a multi-line string keeps its spaces at the end: they are text.
        scan_toml(&formatted, &mut state);
        if state.string.is_some() {
            let kept = &line[line.trim_end().len()..];
            out.push(format!("{formatted}{kept}"));
        } else {
            out.push(formatted);
        }
    }
    join(out, text)
}

/// The data of a TOML document, to compare two layouts of it.
fn toml_data(item: &toml_edit::Item) -> Value {
    match item {
        toml_edit::Item::None => Value::Null,
        toml_edit::Item::Value(value) => toml_value(value),
        toml_edit::Item::Table(table) => Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_string(), toml_data(v)))
                .collect(),
        ),
        toml_edit::Item::ArrayOfTables(tables) => Value::Array(
            tables
                .iter()
                .map(|t| toml_data(&toml_edit::Item::Table(t.clone())))
                .collect(),
        ),
    }
}

fn toml_value(value: &toml_edit::Value) -> Value {
    use toml_edit::Value as V;
    match value {
        V::String(s) => Value::String(s.value().clone()),
        V::Integer(i) => Value::from(*i.value()),
        V::Float(f) => Value::String(format!("{:?}", f.value())),
        V::Boolean(b) => Value::Bool(*b.value()),
        V::Datetime(d) => Value::String(d.value().to_string()),
        V::Array(array) => Value::Array(array.iter().map(toml_value).collect()),
        V::InlineTable(table) => Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_string(), toml_value(v)))
                .collect::<Map<_, _>>(),
        ),
    }
}

/// The JSON of a YAML text: its document, or an array of its documents when it has several.
pub fn yaml_to_json(text: &str) -> Result<String, String> {
    let documents = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    let value = match documents.as_slice() {
        [] => Value::Null,
        [one] => json(one)?,
        many => Value::Array(many.iter().map(json).collect::<Result<_, _>>()?),
    };
    let mut out = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

fn json(yaml: &Yaml) -> Result<Value, String> {
    Ok(match yaml {
        Yaml::Null | Yaml::Alias(_) => Value::Null,
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Integer(i) => Value::from(*i),
        // `.inf` and `.nan` have no JSON number: they stay text.
        Yaml::Real(text) => yaml
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(text.clone()), Value::Number),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Array(items) => Value::Array(items.iter().map(json).collect::<Result<_, _>>()?),
        Yaml::Hash(hash) => {
            let mut object = Map::new();
            let mut merged = Vec::new();
            for (key, value) in hash {
                // `<<: *defaults` brings the keys of another mapping, the ones written win.
                if matches!(key, Yaml::String(k) if k == "<<") {
                    match value {
                        Yaml::Array(maps) => merged.extend(maps),
                        map => merged.push(map),
                    }
                    continue;
                }
                object.insert(key_text(key)?, json(value)?);
            }
            for map in merged {
                if let Value::Object(entries) = json(map)? {
                    for (key, value) in entries {
                        object.entry(key).or_insert(value);
                    }
                }
            }
            Value::Object(object)
        }
        Yaml::BadValue => return Err(tr!("yaml.unknown_alias").into()),
    })
}

/// A key of a JSON object is text: a number or a boolean key is written as it is, a list or a
/// mapping key as its JSON.
fn key_text(key: &Yaml) -> Result<String, String> {
    Ok(match key {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".into(),
        other => json(other)?.to_string(),
    })
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/yaml/format/tests.rs"]
mod tests;
