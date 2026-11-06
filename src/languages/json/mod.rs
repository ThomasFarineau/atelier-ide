//! JSON and YAML schemas, and JSON support: syntax errors, Reformat Code, and what the schema of
//! a file brings — its problems, the completion of its keys and values, and the description of a
//! key under the mouse.
//!
//! A file has a schema when a plugin brings one for its name or its place (the Node.js plugin
//! for `package.json`, GitHub for `.github/workflows/*.yml`), or when it names one: with its
//! `$schema` key in JSON, with a `# yaml-language-server: $schema=…` comment in YAML. The name is a
//! file of the project (a path relative to the file), or the address of a schema a plugin brings.
//! Other addresses are not downloaded.

pub mod parse;
pub mod schema;
pub mod yaml;

use super::{
    Completion, CompletionItem, Diagnostic, DocRef, Hover, ItemKind, Oneshot, Provider, Severity,
    TextEdit, format,
};
use parse::{Node, Spot};
use serde_json::Value;
use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// The schema of some JSON or YAML files, that a plugin brings.
pub struct Schema {
    /// What the tooltips call it.
    pub name: &'static str,
    /// The files it applies to: a name (`package.json`), or the end of a path where `*` stands
    /// for any part of a name (`.github/workflows/*.yml`).
    pub files: &'static [&'static str],
    /// The addresses a file names it by.
    pub urls: &'static [&'static str],
    /// The schema itself, in JSON.
    pub text: &'static str,
    /// Values the plugin finds itself for a place of a JSON file, beyond those of the schema
    /// (the versions of a dependency on the npm registry): from the place, the text typed in
    /// the string and whether the caret is in quotes. None where it has nothing to offer.
    pub values: Option<ValuesHook>,
}

pub type ValuesHook = fn(&[parse::Step], &str, bool) -> Option<Oneshot<Vec<CompletionItem>>>;

impl Schema {
    pub fn applies_to(&self, path: &Path) -> bool {
        let names: Vec<String> = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        self.files.iter().any(|pattern| {
            let parts: Vec<&str> = pattern.split('/').collect();
            names.len() >= parts.len()
                && names[names.len() - parts.len()..]
                    .iter()
                    .zip(&parts)
                    .all(|(name, part)| glob(part, name))
        })
    }
}

/// A name against a pattern where `*` stands for any text.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.strip_prefix(head).is_some_and(|rest| {
            rest.char_indices()
                .map(|(i, _)| i)
                .chain([rest.len()])
                .any(|i| glob(tail, &rest[i..]))
        }),
    }
}

/// The schemas of the plugins that are on, shared by the language service and the provider.
#[derive(Clone, Default)]
pub struct Registry(Arc<Mutex<(u64, Vec<&'static Schema>)>>);

impl Registry {
    /// Sets the schemas; the documents are checked again when they changed.
    pub fn set(&self, schemas: Vec<&'static Schema>) {
        if let Ok(mut inner) = self.0.lock() {
            let same = inner.1.len() == schemas.len()
                && inner
                    .1
                    .iter()
                    .zip(&schemas)
                    .all(|(a, b)| std::ptr::eq(*a, *b));
            if !same {
                inner.0 += 1;
                inner.1 = schemas;
            }
        }
    }

    fn get(&self) -> (u64, Vec<&'static Schema>) {
        self.0
            .lock()
            .map(|inner| (inner.0, inner.1.clone()))
            .unwrap_or_default()
    }
}

/// A document read: its values (several YAML documents in one file), or its syntax error.
struct Read {
    documents: Vec<Node>,
    error: Option<(String, Range<usize>)>,
    /// The schema the text names itself.
    named: Option<String>,
}

fn read(language: &str, text: &str) -> Read {
    if language == "yaml" {
        let named = yaml::schema_comment(text);
        return match yaml::parse(text) {
            Ok(documents) => Read {
                documents,
                error: None,
                named,
            },
            Err(e) => Read {
                documents: Vec::new(),
                error: Some((e.message, e.range)),
                named,
            },
        };
    }
    let clean = parse::clean(text);
    if let Err(error) = serde_json::from_str::<Value>(&clean) {
        let at = position(text, error.line(), error.column());
        let end = text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
        return Read {
            documents: Vec::new(),
            error: Some((error.to_string(), at..end)),
            named: None,
        };
    }
    let documents: Vec<Node> = parse::parse(&clean).into_iter().collect();
    let named = documents
        .first()
        .and_then(|d| d.member("$schema"))
        .and_then(|m| m.value.value().as_str().map(str::to_string));
    Read {
        documents,
        error: None,
        named,
    }
}

struct Document {
    text: String,
    language: String,
    path: Option<PathBuf>,
    /// The schemas the diagnostics were made with.
    generation: u64,
    diagnostics: Arc<Vec<Diagnostic>>,
}

pub struct Json {
    registry: Registry,
    documents: HashMap<u64, Document>,
    /// The schemas of the plugins, read once.
    parsed: HashMap<usize, Arc<Value>>,
}

impl Json {
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            documents: HashMap::new(),
            parsed: HashMap::new(),
        }
    }

    fn plugin_schema(&mut self, schema: &'static Schema) -> Option<Arc<Value>> {
        let key = schema as *const Schema as usize;
        if let Some(value) = self.parsed.get(&key) {
            return Some(value.clone());
        }
        let value = Arc::new(serde_json::from_str(schema.text).ok()?);
        self.parsed.insert(key, Arc::clone(&value));
        Some(value)
    }

    /// The schema of a document, and its name: the one it names, else the one of its place.
    fn schema_of(
        &mut self,
        path: Option<&Path>,
        named: Option<&str>,
    ) -> Option<(String, Arc<Value>)> {
        let (_, schemas) = self.registry.get();
        if let Some(address) = named {
            let address = address.trim_end_matches('#');
            let bare = |a: &str| {
                a.trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_string()
            };
            if let Some(found) = schemas
                .iter()
                .find(|s| s.urls.iter().any(|u| bare(u) == bare(address)))
            {
                return Some((found.name.to_string(), self.plugin_schema(found)?));
            }
            if address.contains("://") {
                return None;
            }
            let file = Path::new(address.trim_start_matches("file:"));
            let file = match path.and_then(Path::parent) {
                Some(dir) if file.is_relative() => dir.join(file),
                _ => file.to_path_buf(),
            };
            let text = std::fs::read_to_string(&file).ok()?;
            let value = serde_json::from_str(&parse::clean(&text)).ok()?;
            let name = file.file_name()?.to_string_lossy().into_owned();
            return Some((name, Arc::new(value)));
        }
        let path = path?;
        let found = schemas.iter().find(|s| s.applies_to(path))?;
        Some((found.name.to_string(), self.plugin_schema(found)?))
    }

    /// The values hook of the plugin schema of a document, found as `schema_of` finds it.
    fn hook_of(&self, path: Option<&Path>, named: Option<&str>) -> Option<ValuesHook> {
        let (_, schemas) = self.registry.get();
        let bare = |a: &str| {
            a.trim_end_matches('#')
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_string()
        };
        let found = match named {
            Some(address) => schemas
                .iter()
                .find(|s| s.urls.iter().any(|u| bare(u) == bare(address))),
            None => schemas
                .iter()
                .find(|s| path.is_some_and(|p| s.applies_to(p))),
        };
        found.and_then(|s| s.values)
    }

    fn check(&mut self, language: &str, text: &str, path: Option<&Path>) -> Vec<Diagnostic> {
        let read = read(language, text);
        let schema = self.schema_of(path, read.named.as_deref());
        // A YAML file without a schema may be a template (Helm, `{{ }}`): not YAML yet.
        if language == "yaml" && schema.is_none() {
            return Vec::new();
        }
        if let Some((message, range)) = read.error {
            return vec![Diagnostic {
                range,
                severity: Severity::Error,
                message,
                source: if language == "yaml" { "YAML" } else { "JSON" }.into(),
            }];
        }
        let Some((_, schema)) = schema else {
            return Vec::new();
        };
        let mut problems = Vec::new();
        for document in &read.documents {
            schema::validate(&schema, document, &schema, &mut problems);
        }
        problems
            .into_iter()
            .map(|p| Diagnostic {
                range: p.range,
                severity: Severity::Warning,
                message: p.message,
                source: "JSON Schema".into(),
            })
            .collect()
    }

    /// The items for a place: the keys the schemas describe, or their values.
    fn items(&self, root: &Value, spot: &Spot, yaml: bool) -> Vec<CompletionItem> {
        let schemas = schema::at(root, &spot.path);
        let mut items = Vec::new();
        if spot.key {
            for (key, s) in schema::properties(&schemas) {
                if spot.present.iter().any(|p| p == key) {
                    continue;
                }
                let types = schema::types(root, s);
                let mut item = CompletionItem::new(key, ItemKind::Property);
                if !types.is_empty() {
                    item = item.detail(types.join(" | "));
                }
                if let Some(doc) = schema::description(root, s) {
                    item = item.doc(doc);
                }
                if spot.in_string.is_none() {
                    let (insert, caret) = if yaml {
                        yaml_key(key, &types)
                    } else {
                        let listed = !schema::values(root, s).iter().all(Value::is_boolean);
                        json_key(key, &types, listed && types.iter().all(|t| t == "string"))
                    };
                    item = item.insert(insert, Some(caret));
                }
                items.push(item);
            }
            return items;
        }
        for s in &schemas {
            for value in schema::values(root, s) {
                let (label, insert) = match (&value, spot.in_string) {
                    (Value::String(v), Some(_)) => (v.clone(), v.clone()),
                    (Value::String(v), None) if yaml => (v.clone(), yaml::plain(v)),
                    (Value::String(v), None) => (v.clone(), value.to_string()),
                    (_, Some(_)) => continue,
                    (other, None) => (other.to_string(), other.to_string()),
                };
                if items.iter().any(|i: &CompletionItem| i.label == label) {
                    continue;
                }
                let mut item = CompletionItem::new(label, ItemKind::Constant);
                if let Some(doc) = schema::description(root, s) {
                    item = item.doc(doc);
                }
                items.push(item.insert(insert, None));
            }
        }
        items
    }
}

/// The byte offset of a 1-based line and column of serde_json.
fn position(text: &str, line: usize, column: usize) -> usize {
    let start: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    let mut at = (start + column.saturating_sub(1)).min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// What a JSON key brings when it is completed outside quotes: `"name": ` and the start of its
/// value, with the place of the caret.
fn json_key(key: &str, types: &[String], listed: bool) -> (String, usize) {
    let head = format!("{}: ", Value::String(key.to_string()));
    let (tail, caret) = match types.first().map(String::as_str) {
        _ if listed => ("\"\"", 1),
        Some("string") => ("\"\"", 1),
        Some("object") => ("{}", 1),
        Some("array") => ("[]", 1),
        _ => ("", 0),
    };
    (format!("{head}{tail}"), head.len() + caret)
}

/// What a YAML key brings: `name: `, or the first line of its block for an object or a list.
fn yaml_key(key: &str, types: &[String]) -> (String, usize) {
    let insert = match types.first().map(String::as_str) {
        Some("object") => format!("{key}:\n  "),
        Some("array") => format!("{key}:\n  - "),
        _ => format!("{key}: "),
    };
    let caret = insert.len();
    (insert, caret)
}

impl Provider for Json {
    fn handles(&self, language: &str) -> bool {
        language == "json" || language == "yaml"
    }

    fn update(&mut self, doc: &DocRef, text: &str, _version: u64) {
        let (generation, _) = self.registry.get();
        let diagnostics = Arc::new(self.check(&doc.language, text, doc.path.as_deref()));
        self.documents.insert(
            doc.key,
            Document {
                text: text.to_string(),
                language: doc.language.clone(),
                path: doc.path.clone(),
                generation,
                diagnostics,
            },
        );
    }

    fn close(&mut self, key: u64) {
        self.documents.remove(&key);
    }

    fn diagnostics(&mut self, key: u64) -> Arc<Vec<Diagnostic>> {
        let (generation, _) = self.registry.get();
        let Some(document) = self.documents.get(&key) else {
            return Arc::default();
        };
        if document.generation == generation {
            return document.diagnostics.clone();
        }
        // A plugin was turned on or off: its schema comes or goes.
        let (language, text, path) = (
            document.language.clone(),
            document.text.clone(),
            document.path.clone(),
        );
        let diagnostics = Arc::new(self.check(&language, &text, path.as_deref()));
        if let Some(document) = self.documents.get_mut(&key) {
            document.generation = generation;
            document.diagnostics = diagnostics.clone();
        }
        diagnostics
    }

    fn complete(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        _explicit: bool,
    ) -> Oneshot<Option<Completion>> {
        let yaml = doc.language == "yaml";
        let named = if yaml {
            yaml::schema_comment(text)
        } else {
            read(&doc.language, text).named
        };
        let hook = self.hook_of(doc.path.as_deref(), named.as_deref());
        let Some((_, root)) = self.schema_of(doc.path.as_deref(), named.as_deref()) else {
            return Oneshot::ready(None);
        };
        let (spot, from) = if yaml {
            match yaml::spot(text, offset) {
                Some(found) => found,
                None => return Oneshot::ready(None),
            }
        } else {
            let spot = parse::spot(&parse::clean(text), offset);
            let word = offset
                - text[..offset]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '@' | '/' | '.'))
                    .map(char::len_utf8)
                    .sum::<usize>();
            let from = spot.in_string.unwrap_or(word);
            (spot, from)
        };
        let from = from.min(offset);
        let found = hook
            .filter(|_| !yaml && !spot.key)
            .and_then(|hook| hook(&spot.path, &text[from..offset], spot.in_string.is_some()));
        let mut items = self.items(&root, &spot, yaml);
        // In YAML, `- ` alone is a key of an item or a value of a list: what the schema says.
        if items.is_empty() && yaml && spot.key {
            let value = Spot { key: false, ..spot };
            items = self.items(&root, &value, yaml);
        }
        if let Some(found) = found {
            // The values of the plugin first, those of the schema after them.
            return super::join(vec![found], None, move |mut got| {
                let mut all = got.pop().flatten().unwrap_or_default();
                all.extend(items);
                (!all.is_empty()).then_some(Completion { from, items: all })
            });
        }
        Oneshot::ready((!items.is_empty()).then_some(Completion { from, items }))
    }

    fn hover(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Option<Hover>> {
        let read = read(&doc.language, text);
        let Some((name, root)) = self.schema_of(doc.path.as_deref(), read.named.as_deref()) else {
            return Oneshot::ready(None);
        };
        let Some((path, member)) = read.documents.iter().find_map(|d| parse::key_at(d, offset))
        else {
            return Oneshot::ready(None);
        };
        let schemas = schema::at(&root, &path);
        let body = schemas
            .iter()
            .find_map(|s| schema::description(&root, s))
            .unwrap_or_default();
        let mut types: Vec<String> = Vec::new();
        for t in schemas.iter().flat_map(|s| schema::types(&root, s)) {
            if !types.contains(&t) {
                types.push(t);
            }
        }
        if body.is_empty() && types.is_empty() {
            return Oneshot::ready(None);
        }
        let mut title = if doc.language == "yaml" {
            member.key.clone()
        } else {
            Value::String(member.key.clone()).to_string()
        };
        if !types.is_empty() {
            title = format!("{title}: {}", types.join(" | "));
        }
        let from = trf!("json.from_schema", schema = name);
        let body = if body.is_empty() {
            from
        } else {
            format!("{body}\n\n{from}")
        };
        Oneshot::ready(Some(Hover {
            range: member.key_range.clone(),
            title,
            body,
        }))
    }

    fn format(&mut self, doc: &DocRef, text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
        Oneshot::ready(match doc.language.as_str() {
            "json" => format::json(text).map(|new| format::edits_between(text, &new)),
            // YAML is formatted by its plugin, while it is on.
            _ => Err(tr!("no_formatter_language").into()),
        })
    }
}

#[cfg(test)]
#[path = "../../../test/languages/json/tests.rs"]
mod tests;
