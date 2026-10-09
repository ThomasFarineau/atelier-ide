//! JavaScript and TypeScript support: syntax errors, completion and hover.
//!
//! The analysis is done in Rust with tree-sitter (`analysis`, `complete`, `hover`), so it works
//! as soon as a file is open, and the built-in tables (`builtins`) document the language and
//! Node.js. When `typescript-language-server` is installed (`server`), it answers with the
//! precision of a type checker and takes over, with the Rust analysis as a fallback.

pub(crate) mod analysis;
use analysis::Dialect;
pub(crate) mod builtins;
pub(crate) mod complete;
pub(crate) mod hover;
pub(crate) mod imports;
pub(crate) mod navigate;
pub(crate) mod rename;
mod server;
pub(crate) mod symbols;

use crate::languages::{
    Completion, CompletionItem, Diagnostic, DocRef, FileEdit, Hover, ItemKind, Location, Oneshot,
    Provider, QuickFix, Severity, Symbol, TextEdit,
    lsp::{self, Client, from_position, to_position},
};
use crate::plugins::builtin::{BuiltinPlugin, Command, Host};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The language id the protocol wants for a file.
fn language_id(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "js" | "mjs" | "cjs" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        _ => None,
    }
}

enum Server {
    NotStarted,
    Missing,
    Running(Arc<Client>),
    Failed(String),
}

struct DocData {
    path: Option<PathBuf>,
    uri: Option<String>,
    language: String,
    text: String,
    version: u64,
    /// The server knows the document.
    opened: bool,
    /// What the diagnostics were computed for: the text version and the server's generation.
    analyzed: Option<(u64, u64)>,
    diagnostics: Arc<Vec<Diagnostic>>,
}

pub struct JavaScript {
    root: PathBuf,
    repaint: Arc<dyn Fn() + Send + Sync>,
    docs: HashMap<u64, DocData>,
    server: Server,
}

impl JavaScript {
    pub fn new(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            root: root.to_path_buf(),
            repaint,
            docs: HashMap::new(),
            server: Server::NotStarted,
        }
    }

    fn client(&self) -> Option<&Arc<Client>> {
        match &self.server {
            Server::Running(client) if !client.exited() => Some(client),
            _ => None,
        }
    }

    /// Executes what the plugin's commands asked for.
    fn poll_control(&mut self) {
        let (install, restart) = server::control(|c| {
            (
                std::mem::take(&mut c.install_requested),
                std::mem::take(&mut c.restart_requested),
            )
        });
        if install {
            server::install();
        }
        if restart {
            self.server = Server::NotStarted;
            for doc in self.docs.values_mut() {
                doc.opened = false;
                doc.analyzed = None;
            }
        }
    }

    fn ensure_server(&mut self) {
        if !matches!(self.server, Server::NotStarted) {
            if let Server::Running(client) = &self.server {
                let failure = client.failure().or_else(|| {
                    client.exited().then(|| {
                        let log = client.log();
                        let last = log.lines().last().unwrap_or("").to_string();
                        if last.is_empty() {
                            "le serveur s'est arrêté".to_string()
                        } else {
                            last
                        }
                    })
                });
                if let Some(message) = failure {
                    server::control(|c| {
                        c.messages
                            .push((format!("Serveur de langage JavaScript : {message}"), true))
                    });
                    self.server = Server::Failed(message);
                }
            }
            return;
        }
        if crate::wsl::parse(&self.root).is_some() {
            self.server = Server::Missing;
            return;
        }
        let Some(launch) = server::launch(&self.root) else {
            self.server = Server::Missing;
            return;
        };
        self.server = match Client::start(&launch, &self.root, self.repaint.clone()) {
            Ok(client) => Server::Running(client),
            Err(e) => Server::Failed(format!("{e:#}")),
        };
    }

    /// Tells the server about the current text of a document.
    fn sync(&mut self, key: u64) {
        let Some(client) = self.client().cloned() else {
            return;
        };
        let Some(doc) = self.docs.get_mut(&key) else {
            return;
        };
        let (Some(uri), Some(path)) = (&doc.uri, &doc.path) else {
            return;
        };
        let Some(language) = language_id(path) else {
            return;
        };
        if doc.opened {
            client.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": doc.version},
                    "contentChanges": [{"text": doc.text}],
                }),
            );
        } else {
            client.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": language, "version": doc.version, "text": doc.text}}),
            );
            doc.opened = true;
        }
    }

    fn native(language: &str) -> bool {
        matches!(language, "js" | "ts")
    }

    /// Asks the server for places (definition or references), the local analysis answering
    /// when there is no server or it knows nothing.
    fn locate(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        method: &str,
        extra: Value,
    ) -> Oneshot<Vec<Location>> {
        let _dialect = analysis::use_dialect(Dialect::of(doc.path.as_deref(), &doc.language));
        let local = if !Self::native(&doc.language) {
            Vec::new()
        } else {
            match method {
                "textDocument/definition" => {
                    navigate::definition(text, offset, doc.path.as_deref())
                }
                "textDocument/typeDefinition" => navigate::type_definition(text, offset),
                "textDocument/references" => navigate::references(text, offset),
                _ => Vec::new(),
            }
        };
        let client = self.client().filter(|c| c.ready()).cloned();
        let data = self.docs.get(&doc.key);
        let (
            Some(client),
            Some(DocData {
                uri: Some(uri),
                opened: true,
                ..
            }),
        ) = (client, data)
        else {
            return Oneshot::ready(local);
        };
        let (line, character) = to_position(text, offset);
        let mut params = json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}});
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            params.extend(extra.clone());
        }
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        let uri = uri.clone();
        client.request(method, params, move |result| {
            let from_server = result
                .map(|r| locations_from_server(&r, &uri, &snapshot))
                .unwrap_or_default();
            slot.fulfill(if from_server.is_empty() {
                local
            } else {
                from_server
            });
        });
        answer
    }
}

fn severity(code: u8) -> Severity {
    match code {
        1 => Severity::Error,
        2 => Severity::Warning,
        3 => Severity::Info,
        _ => Severity::Hint,
    }
}

fn item_kind(code: u64) -> ItemKind {
    match code {
        2 | 4 => ItemKind::Method,
        3 => ItemKind::Function,
        5 | 10 | 20 => ItemKind::Property,
        6 => ItemKind::Variable,
        7 | 22 => ItemKind::Class,
        8 => ItemKind::Interface,
        9 => ItemKind::Module,
        14 | 24 => ItemKind::Keyword,
        15 => ItemKind::Snippet,
        17 => ItemKind::File,
        19 => ItemKind::Folder,
        21 | 12 | 13 => ItemKind::Constant,
        25 => ItemKind::Interface,
        _ => ItemKind::Text,
    }
}

/// Text of a protocol `documentation` or `contents` value.
fn markup_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(map) => map
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Value::Array(items) => items
            .iter()
            .map(markup_text)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Plain text from markdown: links keep their label, emphasis and code marks go.
fn plain(markdown: &str) -> String {
    let mut out = String::new();
    let mut rest = markdown;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match (after.find("]("), after.find(')')) {
            (Some(mid), Some(close)) if mid < close => {
                out.push_str(&after[..mid]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let out = out.replace("**", "").replace('`', "").replace("*@", "@");
    // `*@param*` is a tag in italics: both stars are emphasis.
    out.lines()
        .map(|line| {
            line.split(' ')
                .map(|word| {
                    if word.starts_with('@') {
                        word.trim_end_matches('*')
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// Splits a hover into the signature (the first code block) and the text around it.
fn split_hover(markdown: &str) -> (String, String) {
    let mut title = String::new();
    let mut body = String::new();
    let mut in_code = false;
    let mut first_block = true;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            if !in_code {
                first_block = false;
            }
            continue;
        }
        if in_code && first_block {
            if !title.is_empty() {
                title.push(' ');
            }
            title.push_str(line.trim());
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if title.is_empty() {
        // No code block: the first line is the title.
        let mut lines = markdown.lines();
        title = lines.next().unwrap_or("").to_string();
        body = lines.collect::<Vec<_>>().join("\n");
    }
    (plain(&title), plain(body.trim()))
}

/// Places of a `textDocument/definition` or `references` answer: a location, a list of
/// locations, or links to them. `uri` is the document the request was made in.
fn locations_from_server(result: &Value, uri: &str, snapshot: &str) -> Vec<Location> {
    let list: Vec<&Value> = match result {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![result],
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for item in list {
        let target = item["targetUri"].as_str().or(item["uri"].as_str());
        let range = if item["targetSelectionRange"].is_object() {
            &item["targetSelectionRange"]
        } else if item["targetRange"].is_object() {
            &item["targetRange"]
        } else {
            &item["range"]
        };
        let (Some(target), Some(start), Some(end)) = (
            target,
            range["start"]["line"].as_u64(),
            range["end"]["line"].as_u64(),
        ) else {
            continue;
        };
        let column = |side: &str| range[side]["character"].as_u64().unwrap_or(0) as usize;
        let same = lsp::normalize_uri(target) == lsp::normalize_uri(uri);
        let path = if same { None } else { lsp::uri_to_path(target) };
        if !same && path.is_none() {
            continue;
        }
        let owned;
        let text = if same {
            snapshot
        } else {
            owned = path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .filter(|t| t.len() <= 4_000_000)
                .unwrap_or_default();
            &owned
        };
        let preview = text
            .lines()
            .nth(start as usize)
            .unwrap_or("")
            .trim()
            .chars()
            .take(120)
            .collect();
        out.push(Location {
            path,
            line: start as usize,
            column: column("start"),
            end_line: end as usize,
            end_column: column("end"),
            preview,
        });
    }
    out
}

/// The files and edits of a `WorkspaceEdit`. `uri` is the document the request was made in:
/// its edits come with a path None.
fn workspace_edit(result: &Value, uri: &str, snapshot: &str) -> Vec<FileEdit> {
    let wanted = lsp::normalize_uri(uri);
    let mut per_file: Vec<(String, Vec<Value>)> = Vec::new();
    if let Some(changes) = result["changes"].as_object() {
        for (target, edits) in changes {
            per_file.push((
                target.clone(),
                edits.as_array().cloned().unwrap_or_default(),
            ));
        }
    }
    if let Some(changes) = result["documentChanges"].as_array() {
        for change in changes {
            if let Some(target) = change["textDocument"]["uri"].as_str() {
                per_file.push((
                    target.to_string(),
                    change["edits"].as_array().cloned().unwrap_or_default(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    for (target, edits) in per_file {
        let same = lsp::normalize_uri(&target) == wanted;
        let path = if same {
            None
        } else {
            lsp::uri_to_path(&target)
        };
        if !same && path.is_none() {
            continue;
        }
        let owned;
        let text = if same {
            snapshot
        } else {
            owned = path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default();
            &owned
        };
        let converted: Vec<TextEdit> = edits
            .iter()
            .filter_map(|e| {
                let range = &e["range"];
                let start = from_position(
                    text,
                    range["start"]["line"].as_u64()? as usize,
                    range["start"]["character"].as_u64()? as usize,
                );
                let end = from_position(
                    text,
                    range["end"]["line"].as_u64()? as usize,
                    range["end"]["character"].as_u64()? as usize,
                );
                Some(TextEdit {
                    range: start..end.max(start),
                    new_text: e["newText"].as_str()?.to_string(),
                })
            })
            .collect();
        if !converted.is_empty() {
            out.push(FileEdit {
                path,
                edits: converted,
            });
        }
    }
    out
}

/// The actions of a `textDocument/codeAction` answer that edit the document `uri`.
fn fixes_from_server(result: &Value, uri: &str, snapshot: &str) -> Vec<QuickFix> {
    let Some(actions) = result.as_array() else {
        return Vec::new();
    };
    let wanted = lsp::normalize_uri(uri);
    let edits_of = |edits: &Value| -> Vec<TextEdit> {
        edits
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|e| {
                        let range = &e["range"];
                        let start = from_position(
                            snapshot,
                            range["start"]["line"].as_u64()? as usize,
                            range["start"]["character"].as_u64()? as usize,
                        );
                        let end = from_position(
                            snapshot,
                            range["end"]["line"].as_u64()? as usize,
                            range["end"]["character"].as_u64()? as usize,
                        );
                        Some(TextEdit {
                            range: start..end.max(start),
                            new_text: e["newText"].as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut fixes = Vec::new();
    for action in actions {
        let Some(title) = action["title"].as_str() else {
            continue;
        };
        let edit = &action["edit"];
        let mut edits: Vec<TextEdit> = Vec::new();
        let mut elsewhere = false;
        if let Some(changes) = edit["changes"].as_object() {
            for (target, list) in changes {
                if lsp::normalize_uri(target) == wanted {
                    edits.extend(edits_of(list));
                } else {
                    elsewhere = true;
                }
            }
        }
        if let Some(changes) = edit["documentChanges"].as_array() {
            for change in changes {
                match change["textDocument"]["uri"].as_str() {
                    Some(target) if lsp::normalize_uri(target) == wanted => {
                        edits.extend(edits_of(&change["edits"]))
                    }
                    _ => elsewhere = true,
                }
            }
        }
        // Actions that run a command or touch other files are not offered here.
        if !edits.is_empty() && !elsewhere {
            fixes.push(QuickFix {
                title: title.to_string(),
                edits,
            });
        }
    }
    fixes
}

fn symbol_kind(code: u64) -> ItemKind {
    match code {
        5 | 23 => ItemKind::Class,
        6 | 9 => ItemKind::Method,
        7 | 8 => ItemKind::Property,
        11 => ItemKind::Interface,
        12 => ItemKind::Function,
        13 => ItemKind::Variable,
        14 | 22 | 10 => ItemKind::Constant,
        2 | 3 | 4 => ItemKind::Module,
        _ => ItemKind::Text,
    }
}

/// The structure of a `textDocument/documentSymbol` answer, nested or flat, in document order.
fn symbols_from_server(result: &Value, snapshot: &str) -> Vec<Symbol> {
    fn convert(items: &[Value], depth: usize, snapshot: &str, out: &mut Vec<Symbol>) {
        for item in items {
            let Some(name) = item["name"].as_str() else {
                continue;
            };
            let range = if item["selectionRange"].is_object() {
                &item["selectionRange"]
            } else {
                &item["location"]["range"]
            };
            let (Some(sl), Some(sc), Some(el), Some(ec)) = (
                range["start"]["line"].as_u64(),
                range["start"]["character"].as_u64(),
                range["end"]["line"].as_u64(),
                range["end"]["character"].as_u64(),
            ) else {
                continue;
            };
            let start = from_position(snapshot, sl as usize, sc as usize);
            let end = from_position(snapshot, el as usize, ec as usize).max(start);
            out.push(Symbol {
                name: name.to_string(),
                kind: symbol_kind(item["kind"].as_u64().unwrap_or(0)),
                detail: item["detail"].as_str().unwrap_or("").to_string(),
                range: start..end,
                depth,
            });
            if let Some(children) = item["children"].as_array() {
                convert(children, depth + 1, snapshot, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(items) = result.as_array() {
        convert(items, 0, snapshot, &mut out);
    }
    out
}

fn completion_from_server(result: &Value, text: &str, from: usize) -> Option<Completion> {
    let list = match result {
        Value::Array(items) => items,
        Value::Object(map) => map.get("items")?.as_array()?,
        _ => return None,
    };
    let mut entries: Vec<(&Value, String)> = list
        .iter()
        .map(|item| {
            (
                item,
                item["sortText"]
                    .as_str()
                    .or(item["label"].as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect();
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    let mut items = Vec::new();
    for (index, (item, _)) in entries.into_iter().enumerate().take(1500) {
        let Some(label) = item["label"].as_str() else {
            continue;
        };
        let kind = item_kind(item["kind"].as_u64().unwrap_or(1));
        let mut insert = item["insertText"].as_str().unwrap_or(label).to_string();
        if let Some(edit) = item.get("textEdit") {
            let start = edit["range"]["start"].as_object().and_then(|_| {
                Some(from_position(
                    text,
                    edit["range"]["start"]["line"].as_u64()? as usize,
                    edit["range"]["start"]["character"].as_u64()? as usize,
                ))
            });
            if start == Some(from)
                && let Some(new_text) = edit["newText"].as_str()
            {
                insert = new_text.to_string();
            }
        }
        let mut entry = CompletionItem::new(label, kind).rank(index.min(10_000) as u32);
        // Documentation and signatures come with a second request, when the item is selected.
        if item["documentation"].is_null() && item["detail"].is_null() {
            entry.raw = Some(Arc::new(item.to_string()));
        }
        let detail = item["detail"]
            .as_str()
            .or_else(|| item["labelDetails"]["description"].as_str());
        if let Some(detail) = detail {
            entry = entry.detail(detail);
        }
        let doc = plain(&markup_text(&item["documentation"]));
        if !doc.is_empty() {
            entry = entry.doc(doc);
        }
        // A function gets its parentheses, with the caret inside.
        if matches!(kind, ItemKind::Function | ItemKind::Method)
            && insert == label
            && !text[from..].starts_with('(')
        {
            entry = entry.insert(format!("{label}()"), Some(label.len() + 1));
        } else if insert != label {
            entry = entry.insert(insert, None);
        }
        items.push(entry);
    }
    (!items.is_empty()).then_some(Completion { from, items })
}

impl Provider for JavaScript {
    fn handles(&self, language: &str) -> bool {
        matches!(language, "js" | "ts")
    }

    fn update(&mut self, doc: &DocRef, text: &str, version: u64) {
        self.poll_control();
        self.ensure_server();
        let uri = doc
            .path
            .as_ref()
            .filter(|p| language_id(p).is_some() && crate::wsl::parse(p).is_none())
            .map(|p| lsp::path_to_uri(p));
        let client = self.client().cloned();
        let data = self.docs.entry(doc.key).or_insert_with(|| DocData {
            path: doc.path.clone(),
            uri: None,
            language: doc.language.clone(),
            text: String::new(),
            version: 0,
            opened: false,
            analyzed: None,
            diagnostics: Arc::default(),
        });
        // Saved under another name: the server sees a new document.
        if data.path != doc.path {
            if let (Some(client), Some(old)) = (&client, &data.uri)
                && data.opened
            {
                client.notify(
                    "textDocument/didClose",
                    json!({"textDocument": {"uri": old}}),
                );
            }
            data.opened = false;
            data.path = doc.path.clone();
        }
        data.uri = uri;
        data.language = doc.language.clone();
        data.text = text.to_string();
        data.version = version;
        self.sync(doc.key);
    }

    fn close(&mut self, key: u64) {
        let Some(data) = self.docs.remove(&key) else {
            return;
        };
        if let (Some(client), Some(uri)) = (self.client(), &data.uri)
            && data.opened
        {
            client.notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": uri}}),
            );
        }
    }

    fn diagnostics(&mut self, key: u64) -> Arc<Vec<Diagnostic>> {
        self.poll_control();
        let generation = self.client().map_or(0, |c| c.generation());
        let client = self.client().cloned();
        let Some(data) = self.docs.get_mut(&key) else {
            return Arc::default();
        };
        if data.analyzed == Some((data.version, generation)) {
            return data.diagnostics.clone();
        }
        let mut all: Vec<Diagnostic> = Vec::new();
        if let (Some(client), Some(uri)) = (&client, &data.uri) {
            let (found, _) = client.diagnostics(&lsp::normalize_uri(uri));
            for d in found {
                let start = from_position(&data.text, d.start.0, d.start.1);
                let end = from_position(&data.text, d.end.0, d.end.1).max(start);
                all.push(Diagnostic {
                    range: start..end,
                    severity: severity(d.severity),
                    message: d.message,
                    source: if d.source.is_empty() {
                        "tsserver".into()
                    } else {
                        d.source
                    },
                });
            }
        }
        if Self::native(&data.language) && data.text.len() <= complete::MAX_TEXT {
            let _dialect = analysis::use_dialect(Dialect::of(data.path.as_deref(), &data.language));
            for native in analysis::syntax_errors(&data.text) {
                // The server already says it, better.
                let covered = all.iter().any(|d| {
                    d.severity == Severity::Error
                        && d.range.start <= native.range.end
                        && native.range.start <= d.range.end
                });
                if !covered {
                    all.push(native);
                }
            }
        }
        all.sort_by_key(|d| (d.range.start, d.severity));
        data.diagnostics = Arc::new(all);
        data.analyzed = Some((data.version, generation));
        data.diagnostics.clone()
    }

    fn complete(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        explicit: bool,
    ) -> Oneshot<Option<Completion>> {
        let _dialect = analysis::use_dialect(Dialect::of(doc.path.as_deref(), &doc.language));
        let native = if Self::native(&doc.language) {
            complete::complete(text, offset, explicit, doc.path.as_deref())
        } else {
            None
        };
        let client = self.client().filter(|c| c.ready()).cloned();
        let data = self.docs.get(&doc.key);
        let (
            Some(client),
            Some(DocData {
                uri: Some(uri),
                opened: true,
                ..
            }),
        ) = (client, data)
        else {
            return Oneshot::ready(native);
        };
        let offset = offset.min(text.len());
        let from = complete::word_start(text, offset);
        // After a dot only the server knows the type; in a string it knows the paths.
        let typed = &text[from..offset];
        if typed.is_empty() && !explicit && !text[..from].ends_with(['.', '/']) {
            return Oneshot::ready(None);
        }
        let (line, character) = to_position(text, offset);
        let trigger = text[..from]
            .chars()
            .next_back()
            .filter(|c| matches!(c, '.' | '/' | '"' | '\'' | '`' | '<' | '@' | '#'));
        let context = match trigger {
            Some(c) if typed.is_empty() => {
                json!({"triggerKind": 2, "triggerCharacter": c.to_string()})
            }
            _ => json!({"triggerKind": 1}),
        };
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request(
            "textDocument/completion",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}, "context": context}),
            move |result| {
                let from_server = result.ok().and_then(|r| completion_from_server(&r, &snapshot, from));
                slot.fulfill(from_server.or(native));
            },
        );
        answer
    }

    fn hover(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Option<Hover>> {
        let _dialect = analysis::use_dialect(Dialect::of(doc.path.as_deref(), &doc.language));
        let native = if Self::native(&doc.language) {
            hover::hover(text, offset)
        } else {
            None
        };
        let client = self.client().filter(|c| c.ready()).cloned();
        let data = self.docs.get(&doc.key);
        let (
            Some(client),
            Some(DocData {
                uri: Some(uri),
                opened: true,
                ..
            }),
        ) = (client, data)
        else {
            return Oneshot::ready(native);
        };
        let (line, character) = to_position(text, offset);
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
            move |result| {
                let from_server = result.ok().and_then(|r| {
                    let markdown = markup_text(&r["contents"]);
                    if markdown.trim().is_empty() {
                        return None;
                    }
                    let (title, body) = split_hover(&markdown);
                    let range = match (r["range"]["start"].as_object(), r["range"]["end"].as_object()) {
                        (Some(_), Some(_)) => {
                            let start = from_position(&snapshot, r["range"]["start"]["line"].as_u64()? as usize, r["range"]["start"]["character"].as_u64()? as usize);
                            let end = from_position(&snapshot, r["range"]["end"]["line"].as_u64()? as usize, r["range"]["end"]["character"].as_u64()? as usize);
                            start..end
                        }
                        _ => offset..offset,
                    };
                    Some(Hover { range, title, body })
                });
                slot.fulfill(from_server.or(native));
            },
        );
        answer
    }

    fn rename(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        new_name: &str,
    ) -> Oneshot<Result<Vec<FileEdit>, String>> {
        let answer = Oneshot::pending();
        let client = self.client().filter(|c| c.ready()).cloned();
        let target = self
            .docs
            .get(&doc.key)
            .and_then(|d| d.uri.clone().filter(|_| d.opened));
        let local = Self::native(&doc.language);
        let dialect = Dialect::of(doc.path.as_deref(), &doc.language);
        let (text, path, root, repaint) = (
            text.to_string(),
            doc.path.clone(),
            self.root.clone(),
            self.repaint.clone(),
        );
        let new_name = new_name.to_string();
        let slot = answer.clone();
        // The scan of the project for importers takes a while: not on the interface thread.
        std::thread::spawn(move || {
            let _dialect = analysis::use_dialect(dialect);
            let native = if local {
                rename::rename(&text, offset, &new_name, path.as_deref(), &root)
            } else {
                Err("Renommer n'est pas pris en charge sans le serveur de langage".to_string())
            };
            // A name the local analysis refuses (invalid, already used) is refused for good.
            let (Some(client), Some(uri)) = (client, target) else {
                slot.fulfill(native);
                repaint();
                return;
            };
            if local
                && native.as_ref().is_err_and(|e| {
                    e.contains("n'est pas un nom valide") || e.contains("existe déjà")
                })
            {
                slot.fulfill(native);
                repaint();
                return;
            }
            let (line, character) = to_position(&text, offset);
            let params = json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}, "newName": new_name});
            let uri_for_answer = uri.clone();
            client.request("textDocument/rename", params, move |result| {
                let plan = result
                    .map(|r| workspace_edit(&r, &uri_for_answer, &text))
                    .unwrap_or_default();
                slot.fulfill(if plan.is_empty() { native } else { Ok(plan) });
                repaint();
            });
        });
        answer
    }

    fn quick_fixes(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<QuickFix>> {
        let answer = Oneshot::pending();
        let client = self.client().filter(|c| c.ready()).cloned();
        let target = self
            .docs
            .get(&doc.key)
            .and_then(|d| d.uri.clone().filter(|_| d.opened));
        let local = Self::native(&doc.language);
        let (text, path, root, repaint) = (
            text.to_string(),
            doc.path.clone(),
            self.root.clone(),
            self.repaint.clone(),
        );
        let dialect = Dialect::of(doc.path.as_deref(), &doc.language);
        let slot = answer.clone();
        let diagnostics: Vec<Value> = match (&client, &target) {
            (Some(client), Some(uri)) => {
                let (found, _) = client.diagnostics(&lsp::normalize_uri(uri));
                let here = to_position(&text, offset);
                found
                    .into_iter()
                    .filter(|d| d.start <= here && here <= d.end)
                    .map(|d| d.raw)
                    .collect()
            }
            _ => Vec::new(),
        };
        // Scanning the project for exports takes a while: not on the interface thread.
        std::thread::spawn(move || {
            let _dialect = analysis::use_dialect(dialect);
            let native = if local {
                imports::import_fixes(&text, offset, path.as_deref(), &root)
            } else {
                Vec::new()
            };
            let (Some(client), Some(uri)) = (client, target) else {
                slot.fulfill(native);
                repaint();
                return;
            };
            let (line, character) = to_position(&text, offset);
            let params = json!({
                "textDocument": {"uri": uri},
                "range": {"start": {"line": line, "character": character}, "end": {"line": line, "character": character}},
                "context": {"diagnostics": diagnostics, "triggerKind": 1},
            });
            let uri_for_answer = uri.clone();
            client.request("textDocument/codeAction", params, move |result| {
                let from_server = result
                    .map(|r| fixes_from_server(&r, &uri_for_answer, &text))
                    .unwrap_or_default();
                slot.fulfill(if from_server.is_empty() {
                    native
                } else {
                    from_server
                });
                repaint();
            });
        });
        answer
    }

    fn definition(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, "textDocument/definition", json!({}))
    }

    fn type_definition(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, "textDocument/typeDefinition", json!({}))
    }

    fn implementation(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, "textDocument/implementation", json!({}))
    }

    fn highlights(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<std::ops::Range<usize>>> {
        let _dialect = analysis::use_dialect(Dialect::of(doc.path.as_deref(), &doc.language));
        let local = if Self::native(&doc.language) {
            navigate::highlights(text, offset)
        } else {
            Vec::new()
        };
        let client = self.client().filter(|c| c.ready()).cloned();
        let data = self.docs.get(&doc.key);
        let (
            Some(client),
            Some(DocData {
                uri: Some(uri),
                opened: true,
                ..
            }),
        ) = (client, data)
        else {
            return Oneshot::ready(local);
        };
        let (line, character) = to_position(text, offset);
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request(
            "textDocument/documentHighlight",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
            move |result| {
                let places: Vec<std::ops::Range<usize>> = result
                    .ok()
                    .and_then(|r| r.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|h| {
                        let range = &h["range"];
                        let start = from_position(&snapshot, range["start"]["line"].as_u64()? as usize, range["start"]["character"].as_u64()? as usize);
                        let end = from_position(&snapshot, range["end"]["line"].as_u64()? as usize, range["end"]["character"].as_u64()? as usize);
                        Some(start..end.max(start))
                    })
                    .collect();
                slot.fulfill(if places.is_empty() { local } else { places });
            },
        );
        answer
    }

    fn symbols(&mut self, doc: &DocRef, text: &str) -> Oneshot<Vec<Symbol>> {
        let _dialect = analysis::use_dialect(Dialect::of(doc.path.as_deref(), &doc.language));
        let local = if Self::native(&doc.language) {
            symbols::symbols(text)
        } else {
            Vec::new()
        };
        let client = self.client().filter(|c| c.ready()).cloned();
        let data = self.docs.get(&doc.key);
        let (
            Some(client),
            Some(DocData {
                uri: Some(uri),
                opened: true,
                ..
            }),
        ) = (client, data)
        else {
            return Oneshot::ready(local);
        };
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
            move |result| {
                let from_server = result
                    .map(|r| symbols_from_server(&r, &snapshot))
                    .unwrap_or_default();
                slot.fulfill(if from_server.is_empty() {
                    local
                } else {
                    from_server
                });
            },
        );
        answer
    }

    fn references(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        self.locate(
            doc,
            text,
            offset,
            "textDocument/references",
            json!({"context": {"includeDeclaration": false}}),
        )
    }

    fn resolve(&mut self, _doc: &DocRef, item: &CompletionItem) -> Oneshot<Option<CompletionItem>> {
        let client = self.client().filter(|c| c.ready()).cloned();
        let raw = item
            .raw
            .as_ref()
            .and_then(|r| serde_json::from_str::<Value>(r).ok());
        let (Some(client), Some(raw)) = (client, raw) else {
            return Oneshot::ready(None);
        };
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let mut item = item.clone();
        client.request("completionItem/resolve", raw, move |result| {
            let updated = result.ok().map(|resolved| {
                if let Some(detail) = resolved["detail"].as_str() {
                    item.detail = Some(plain(detail));
                }
                let doc = plain(&markup_text(&resolved["documentation"]));
                if !doc.is_empty() {
                    item.doc = Some(doc);
                }
                item.raw = None;
                item
            });
            slot.fulfill(updated);
        });
        answer
    }

    fn status(&self) -> String {
        if server::control(|c| c.installing) {
            return "Installation du serveur JS…".into();
        }
        match &self.server {
            Server::NotStarted => String::new(),
            Server::Running(client) if client.ready() => "tsserver".into(),
            Server::Running(_) => "tsserver…".into(),
            Server::Missing => "JS : analyse locale".into(),
            Server::Failed(reason) => format!(
                "JS : serveur en échec ({})",
                reason.chars().take(60).collect::<String>()
            ),
        }
    }
}

/// The commands of JavaScript support in Search Everywhere.
#[derive(Default)]
pub struct JavaScriptPlugin;

impl BuiltinPlugin for JavaScriptPlugin {
    fn id(&self) -> &'static str {
        "javascript"
    }
    fn name(&self) -> &'static str {
        "JavaScript"
    }
    fn description(&self) -> &'static str {
        "Complétion, infobulles et erreurs de syntaxe pour JavaScript, par analyse locale ; avec typescript-language-server (installé dans ~/.atelier/lsp) pour la précision de TypeScript : types, JSDoc, node_modules."
    }
    fn icon(&self) -> &'static str {
        "settings"
    }

    fn update(&mut self, _ctx: &eframe::egui::Context, host: &mut dyn Host) {
        for (text, error) in server::control(|c| std::mem::take(&mut c.messages)) {
            host.balloon("JavaScript", text, error);
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            (
                "js.install",
                "JavaScript : installer le serveur de langage (typescript-language-server)",
            ),
            (
                "js.restart",
                "JavaScript : redémarrer le serveur de langage",
            ),
        ]
        .into_iter()
        .map(|(id, label)| Command {
            id,
            label: label.into(),
        })
        .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "js.install" => {
                if !server::node_available() {
                    host.balloon("JavaScript", "Node.js est introuvable : installez-le pour utiliser le serveur de langage".into(), true);
                    return;
                }
                host.notify("Installation du serveur de langage JavaScript…".into());
                server::control(|c| c.install_requested = true);
            }
            "js.restart" => server::control(|c| c.restart_requested = true),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_markdown_is_split_into_signature_and_text() {
        let markdown = "```typescript\nfunction add(a: number, b: number): number\n```\n\nAdds **two** [numbers](https://x.y).\n\n*@param* `a` — first";
        let (title, body) = split_hover(markdown);
        assert_eq!(title, "function add(a: number, b: number): number");
        assert!(body.starts_with("Adds two numbers."), "{body}");
        assert!(body.contains("@param a — first"), "{body}");
        let (title, body) = split_hover("Just a sentence.\nMore.");
        assert_eq!(
            (title.as_str(), body.as_str()),
            ("Just a sentence.", "More.")
        );
    }

    #[test]
    fn server_completions_become_items_in_server_order() {
        let text = "const x = [];\nx.pu";
        let result = json!({"isIncomplete": false, "items": [
            {"label": "push", "kind": 2, "sortText": "1", "detail": "(...items: any[]): number", "documentation": {"kind": "markdown", "value": "Appends **items**."}},
            {"label": "length", "kind": 10, "sortText": "0"},
            {"label": "toString", "kind": 2, "sortText": "2", "insertText": "toString"},
        ]});
        let from = text.len() - 2;
        let completion = completion_from_server(&result, text, from).unwrap();
        let labels: Vec<&str> = completion.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["length", "push", "toString"], "sorted by sortText");
        assert_eq!(completion.from, from);
        let push = &completion.items[1];
        assert_eq!(push.insert, "push()");
        assert_eq!(push.caret, Some(5));
        assert_eq!(push.doc.as_deref(), Some("Appends items."));
        assert_eq!(completion.items[0].kind, ItemKind::Property);
        assert!(completion_from_server(&json!({"items": []}), text, from).is_none());
    }

    #[test]
    fn the_provider_falls_back_to_the_local_analysis_without_a_server() {
        let mut provider = JavaScript::new(Path::new("."), Arc::new(|| {}));
        provider.server = Server::Missing;
        let doc = DocRef {
            key: 1,
            path: None,
            language: "js".into(),
        };
        let text = "const total = 5;\nconsole.log(tot";
        provider.update(&doc, text, 1);
        let completion = provider
            .complete(&doc, text, text.len(), false)
            .take()
            .unwrap()
            .unwrap();
        assert!(completion.items.iter().any(|i| i.label == "total"));
        let broken = "const a = ;\n";
        provider.update(&doc, broken, 2);
        assert_eq!(provider.diagnostics(1).len(), 1);
        assert_eq!(provider.status(), "JS : analyse locale");
        let info = provider
            .hover(&doc, "const total = 5;\ntotal;", 18)
            .take()
            .unwrap()
            .unwrap();
        assert_eq!(info.title, "const total: number");
    }

    fn wait_for(
        provider: &mut JavaScript,
        what: &str,
        mut done: impl FnMut(&mut JavaScript) -> bool,
    ) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !done(provider) {
            assert!(
                std::time::Instant::now() < deadline,
                "timeout waiting for {what}: {}",
                provider.status()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Talks to a real typescript-language-server when one is installed (here, or in
    /// `ATELIER_LSP_DIR`); otherwise there is nothing to check.
    #[test]
    fn the_real_server_answers_completion_hover_and_diagnostics() {
        let root = std::env::temp_dir().join(format!("atelier-js-lsp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        if server::launch(&root).is_none() || !server::node_available() {
            return;
        }
        let file = root.join("a.js");
        let text = "/** Adds.
 * @param {number} a
 * @param {number} b
 */
function add(a, b) { return a + b; }
const greeting = 'x';
greeting.toUp;
add(1, 2);
let broken = ;
";
        std::fs::write(&file, text).unwrap();
        let mut provider = JavaScript::new(&root, Arc::new(|| {}));
        let doc = DocRef {
            key: 7,
            path: Some(file.clone()),
            language: "js".into(),
        };
        provider.update(&doc, text, 1);
        wait_for(&mut provider, "the server", |p| {
            p.client().is_some_and(|c| c.ready())
        });
        provider.update(&doc, text, 2);

        let offset = text.find("greeting.toUp").unwrap() + "greeting.toUp".len();
        let answer = provider.complete(&doc, text, offset, false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let completion = loop {
            if let Some(result) = answer.take() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline, "no completion");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let completion = completion.expect("completion items");
        let upper = completion
            .items
            .iter()
            .find(|i| i.label == "toUpperCase")
            .unwrap_or_else(|| {
                panic!(
                    "no toUpperCase in {} items: {:?}",
                    completion.items.len(),
                    completion
                        .items
                        .iter()
                        .take(20)
                        .map(|i| i.label.clone())
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(completion.from, offset - 4);
        assert_eq!(upper.insert, "toUpperCase()");

        let at = text.find("add(1, 2)").unwrap();
        let answer = provider.hover(&doc, text, at);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let info = loop {
            if let Some(result) = answer.take() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline, "no hover");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let info = info.expect("a hover");
        assert!(
            info.title.contains("add(a: number, b: number)"),
            "{}",
            info.title
        );

        wait_for(&mut provider, "diagnostics", |p| {
            !p.diagnostics(7).is_empty()
        });
        let diagnostics = provider.diagnostics(7);
        assert!(
            diagnostics.iter().any(|d| d.severity == Severity::Error),
            "{diagnostics:?}"
        );
        assert_eq!(provider.status(), "tsserver");
        drop(provider);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn wait_answer<T>(answer: &Oneshot<T>, what: &str) -> T {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if let Some(value) = answer.take() {
                return value;
            }
            assert!(std::time::Instant::now() < deadline, "no answer for {what}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Definition, usages, structure and import actions from a real server.
    #[test]
    fn the_real_server_navigates_and_offers_imports() {
        let root = std::env::temp_dir().join(format!("atelier-js-lsp-nav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        if server::launch(&root).is_none() || !server::node_available() {
            return;
        }
        std::fs::write(root.join("jsconfig.json"), "{}").unwrap();
        std::fs::write(
            root.join("util.js"),
            "export function helper() { return 1; }
",
        )
        .unwrap();
        let file = root.join("main.js");
        let text = "function add(a, b) { return a + b; }
const total = add(1, 2);
console.log(total, add(3, 4));
helper();
";
        std::fs::write(&file, text).unwrap();
        let mut provider = JavaScript::new(&root, Arc::new(|| {}));
        let doc = DocRef {
            key: 9,
            path: Some(file.clone()),
            language: "js".into(),
        };
        provider.update(&doc, text, 1);
        wait_for(&mut provider, "the server", |p| {
            p.client().is_some_and(|c| c.ready())
        });
        provider.update(&doc, text, 2);

        // Definition of the first use of `add`.
        let at = text.find("add(1, 2)").unwrap() + 1;
        let found = wait_answer(&provider.definition(&doc, text, at), "definition");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].line, found[0].column), (0, 9));

        // Usages of `add`, from the declaration.
        let declared = text.find("add(a").unwrap() + 1;
        let usages = wait_answer(&provider.references(&doc, text, declared), "references");
        assert_eq!(
            usages.iter().map(|l| l.line).collect::<Vec<_>>(),
            [1, 2],
            "{usages:?}"
        );

        // The structure.
        let symbols = wait_answer(&provider.symbols(&doc, text), "symbols");
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "add" && s.kind == ItemKind::Function),
            "{symbols:?}"
        );
        assert!(symbols.iter().any(|s| s.name == "total"), "{symbols:?}");

        // The highlight of a name.
        let places = wait_answer(&provider.highlights(&doc, text, at), "highlights");
        assert_eq!(places.len(), 3, "the declaration and two uses: {places:?}");

        // Import `helper` from util.js.
        let at = text.find("helper();").unwrap() + 1;
        let fixes = wait_answer(&provider.quick_fixes(&doc, text, at), "quick fixes");
        assert!(
            fixes
                .iter()
                .any(|f| f.title.to_lowercase().contains("import") && f.title.contains("util")),
            "{fixes:?}"
        );
        drop(provider);
        let _ = std::fs::remove_dir_all(&root);
    }
}
