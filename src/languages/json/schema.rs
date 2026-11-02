//! What a JSON Schema says of a document: the problems of its values, and the keys and values it
//! expects at a place. The common part of the drafts is read: `type`, `enum`, `const`,
//! `properties`, `patternProperties`, `additionalProperties`, `required`, `items`, `pattern`,
//! the lengths and bounds, `allOf`, `anyOf`, `oneOf` and local `$ref`s.

use super::parse::{Kind, Node, Step};
use serde_json::Value;
use std::ops::Range;

/// A problem of a value: where it is, and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub range: Range<usize>,
    pub message: String,
}

/// The schema a `$ref` leads to (`#/definitions/person`), else the schema itself.
pub fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let mut schema = schema;
    // A loop of references stops somewhere.
    for _ in 0..32 {
        let Some(target) = schema.get("$ref").and_then(Value::as_str) else {
            break;
        };
        match target
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
        {
            Some(next) => schema = next,
            None => break,
        }
    }
    schema
}

/// The schema, the one its `$ref` leads to, and the ones it combines with `allOf`, `anyOf` and
/// `oneOf`: all of them may say something of the value. A schema with a `$ref` comes first, for
/// the description written next to it.
fn expand<'a>(root: &'a Value, schema: &'a Value, out: &mut Vec<&'a Value>) {
    if out.len() > 256 || out.iter().any(|s| std::ptr::eq(*s, schema)) {
        return;
    }
    out.push(schema);
    let target = resolve(root, schema);
    if !std::ptr::eq(target, schema) {
        expand(root, target, out);
        return;
    }
    for combinator in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(schemas)) = schema.get(combinator) {
            for s in schemas {
                expand(root, s, out);
            }
        }
    }
}

/// The schemas of a key of an object.
fn of_key<'a>(schema: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    if let Some(s) = schema.get("properties").and_then(|p| p.get(key)) {
        out.push(s);
        return;
    }
    let mut matched = false;
    if let Some(Value::Object(patterns)) = schema.get("patternProperties") {
        for (pattern, s) in patterns {
            if regex::Regex::new(pattern).is_ok_and(|r| r.is_match(key)) {
                out.push(s);
                matched = true;
            }
        }
    }
    if !matched && let Some(s) = schema.get("additionalProperties").filter(|s| s.is_object()) {
        out.push(s);
    }
}

/// The schemas of an item of an array.
fn of_item(schema: &Value, index: usize) -> Option<&Value> {
    match (schema.get("prefixItems"), schema.get("items")) {
        (Some(Value::Array(prefix)), _) if index < prefix.len() => Some(&prefix[index]),
        (_, Some(Value::Array(tuple))) => tuple.get(index).or(schema.get("additionalItems")),
        (_, Some(items)) if items.is_object() => Some(items),
        _ => None,
    }
}

/// The schemas that apply at a place of the document.
pub fn at<'a>(root: &'a Value, path: &[Step]) -> Vec<&'a Value> {
    let mut schemas = Vec::new();
    expand(root, root, &mut schemas);
    for step in path {
        let mut next = Vec::new();
        for schema in &schemas {
            match step {
                Step::Key(key) => of_key(schema, key, &mut next),
                Step::Index(index) => next.extend(of_item(schema, *index)),
            }
        }
        schemas = Vec::new();
        for s in next {
            expand(root, s, &mut schemas);
        }
    }
    schemas
}

/// The keys the schemas describe, in their order, each with its schema.
pub fn properties<'a>(schemas: &[&'a Value]) -> Vec<(&'a str, &'a Value)> {
    let mut out: Vec<(&str, &Value)> = Vec::new();
    for schema in schemas {
        if let Some(Value::Object(properties)) = schema.get("properties") {
            for (key, s) in properties {
                if !out.iter().any(|(k, _)| k == key) {
                    out.push((key, s));
                }
            }
        }
    }
    out
}

/// What a schema says of its value, for a tooltip.
pub fn description(root: &Value, schema: &Value) -> Option<String> {
    let mut all = Vec::new();
    expand(root, schema, &mut all);
    all.iter().find_map(|s| {
        ["markdownDescription", "description", "title"]
            .iter()
            .find_map(|k| s.get(*k).and_then(Value::as_str))
            .map(str::to_string)
    })
}

/// The types a schema allows, as it writes them; empty when it does not say.
pub fn types(root: &Value, schema: &Value) -> Vec<String> {
    let mut all = Vec::new();
    expand(root, schema, &mut all);
    let mut out: Vec<String> = Vec::new();
    for s in all {
        let named: Vec<&str> = match s.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
            _ if s.get("properties").is_some() => vec!["object"],
            _ => Vec::new(),
        };
        for t in named {
            if !out.iter().any(|o| o == t) {
                out.push(t.to_string());
            }
        }
    }
    out
}

/// The values a schema lists (`enum`, `const`, and `true` and `false` for a boolean).
pub fn values(root: &Value, schema: &Value) -> Vec<Value> {
    let mut all = Vec::new();
    expand(root, schema, &mut all);
    let mut out: Vec<Value> = Vec::new();
    let mut add = |v: &Value| {
        if !out.contains(v) {
            out.push(v.clone());
        }
    };
    for s in &all {
        if let Some(Value::Array(values)) = s.get("enum") {
            values.iter().for_each(&mut add);
        }
        if let Some(value) = s.get("const") {
            add(value);
        }
    }
    if types(root, schema).iter().any(|t| t == "boolean") {
        add(&Value::Bool(true));
        add(&Value::Bool(false));
    }
    out
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn has_type(value: &Value, wanted: &str) -> bool {
    let actual = type_name(value);
    actual == wanted
        || wanted == "number" && actual == "integer"
        || wanted == "integer" && value.as_f64().is_some_and(|f| f.fract() == 0.0)
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(40) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

/// The problems of `node` against `schema`.
pub fn validate(root: &Value, node: &Node, schema: &Value, out: &mut Vec<Problem>) {
    let schema = resolve(root, schema);
    if schema == &Value::Bool(false) {
        out.push(Problem {
            range: node.range.clone(),
            message: tr!("json.value_not_allowed").into(),
        });
        return;
    }
    if let Some(Value::Array(schemas)) = schema.get("allOf") {
        for s in schemas {
            validate(root, node, s, out);
        }
    }
    for combinator in ["anyOf", "oneOf"] {
        if let Some(Value::Array(schemas)) = schema.get(combinator) {
            // The problems of the branch that fits best, when none fits.
            let mut best: Option<Vec<Problem>> = None;
            for s in schemas {
                let mut problems = Vec::new();
                validate(root, node, s, &mut problems);
                if best.as_ref().is_none_or(|b| problems.len() < b.len()) {
                    best = Some(problems);
                }
            }
            out.extend(best.unwrap_or_default());
        }
    }
    let value = node.value();
    let wanted: Vec<&str> = match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !wanted.is_empty() && !wanted.iter().any(|t| has_type(&value, t)) {
        out.push(Problem {
            range: node.range.clone(),
            message: trf!(
                "json.incompatible_type",
                expected = wanted.join(", "),
                actual = type_name(&value)
            ),
        });
        return;
    }
    if let Some(Value::Array(allowed)) = schema.get("enum")
        && !allowed.contains(&value)
    {
        let list: Vec<String> = allowed.iter().take(12).map(short).collect();
        out.push(Problem {
            range: node.range.clone(),
            message: trf!("json.value_one_of", values = list.join(", ")),
        });
    }
    if let Some(expected) = schema.get("const")
        && expected != &value
    {
        out.push(Problem {
            range: node.range.clone(),
            message: trf!("json.value_must_be", value = short(expected)),
        });
    }
    match &node.kind {
        Kind::Object(members) => object(root, node, members, schema, out),
        Kind::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                if let Some(s) = of_item(schema, i) {
                    validate(root, item, s, out);
                }
            }
            let count = items.len() as u64;
            bounds(node, count, schema, "minItems", "maxItems", out);
        }
        Kind::Scalar(Value::String(text)) => {
            if let Some(pattern) = schema.get("pattern").and_then(Value::as_str)
                && let Ok(regex) = regex::Regex::new(pattern)
                && !regex.is_match(text)
            {
                out.push(Problem {
                    range: node.range.clone(),
                    message: trf!("json.pattern_mismatch", pattern = pattern),
                });
            }
            let count = text.chars().count() as u64;
            bounds(node, count, schema, "minLength", "maxLength", out);
        }
        Kind::Scalar(Value::Number(n)) => {
            let n = n.as_f64().unwrap_or_default();
            let low = schema.get("minimum").and_then(Value::as_f64);
            let high = schema.get("maximum").and_then(Value::as_f64);
            if low.is_some_and(|l| n < l) || high.is_some_and(|h| n > h) {
                let (low, high) = (
                    low.map_or("…".into(), |l| l.to_string()),
                    high.map_or("…".into(), |h| h.to_string()),
                );
                out.push(Problem {
                    range: node.range.clone(),
                    message: trf!("json.out_of_range", low = low, high = high),
                });
            }
        }
        Kind::Scalar(_) => {}
    }
}

/// A count (of characters or items) out of the bounds a schema sets.
fn bounds(node: &Node, count: u64, schema: &Value, min: &str, max: &str, out: &mut Vec<Problem>) {
    let low = schema.get(min).and_then(Value::as_u64);
    let high = schema.get(max).and_then(Value::as_u64);
    if low.is_some_and(|l| count < l) || high.is_some_and(|h| count > h) {
        let (low, high) = (
            low.map_or("…".into(), |l| l.to_string()),
            high.map_or("…".into(), |h| h.to_string()),
        );
        out.push(Problem {
            range: node.range.clone(),
            message: trf!("json.length_out_of_range", low = low, high = high),
        });
    }
}

fn object(
    root: &Value,
    node: &Node,
    members: &[super::parse::Member],
    schema: &Value,
    out: &mut Vec<Problem>,
) {
    if let Some(Value::Array(required)) = schema.get("required") {
        let missing: Vec<&str> = required
            .iter()
            .filter_map(Value::as_str)
            .filter(|k| !members.iter().any(|m| m.key == *k))
            .collect();
        if !missing.is_empty() {
            // On the opening brace, as IntelliJ shows it.
            out.push(Problem {
                range: node.range.start..node.range.start + 1,
                message: trf!("json.missing_properties", keys = missing.join(", ")),
            });
        }
    }
    let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
    for member in members {
        let mut schemas = Vec::new();
        of_key(schema, &member.key, &mut schemas);
        if schemas.is_empty() && closed && member.key != "$schema" {
            out.push(Problem {
                range: member.key_range.clone(),
                message: trf!("json.property_not_allowed", key = member.key),
            });
        }
        for s in schemas {
            validate(root, &member.value, s, out);
        }
    }
}
