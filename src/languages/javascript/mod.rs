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
    Completion, CompletionItem, Diagnostic, DocRef, FileEdit, Hover, Location, Oneshot, Provider,
    QuickFix, Severity, Symbol, TextEdit,
    format::{apply as apply_edits, edits_between},
    lsp::{
        self, Client,
        convert::{
            completion_from_server, fixes_from_server, locations_from_server, markup_text, plain,
            severity, split_hover, symbols_from_server, workspace_edit,
        },
        from_position, to_position,
    },
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
                            tr!("javascript.server_stopped").to_string()
                        } else {
                            last
                        }
                    })
                });
                if let Some(message) = failure {
                    server::control(|c| {
                        c.messages.push((
                            trf!("javascript.javascript_language_server", message = message),
                            true,
                        ))
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

impl Provider for JavaScript {
    fn handles(&self, language: &str) -> bool {
        matches!(language, "js" | "ts")
    }

    fn workspace_diagnostics(&mut self) -> Vec<(PathBuf, Vec<lsp::ServerDiagnostic>)> {
        let Some(client) = self.client() else {
            return Vec::new();
        };
        client
            .all_diagnostics()
            .into_iter()
            .filter_map(|(uri, list)| Some((lsp::uri_to_path(&uri)?, list)))
            .collect()
    }

    fn lsp_target(&mut self, doc: &DocRef) -> Option<(Arc<lsp::Client>, String)> {
        let client = self.client().filter(|c| c.ready())?.clone();
        let data = self.docs.get(&doc.key)?;
        Some((client, data.uri.clone().filter(|_| data.opened)?))
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
                Err(tr!("javascript.rename_not_supported_without_language").to_string())
            };
            // A name the local analysis refuses (invalid, already used) is refused for good.
            let (Some(client), Some(uri)) = (client, target) else {
                slot.fulfill(native);
                repaint();
                return;
            };
            if local
                && native.as_ref().is_err_and(|e| {
                    // In either language of the interface.
                    [
                        "n'est pas un nom valide",
                        "existe déjà",
                        "is not a valid name",
                        "already exists",
                    ]
                    .iter()
                    .any(|refusal| e.contains(refusal))
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

    /// tsserver formats with the indentation the file already uses.
    fn format(&mut self, doc: &DocRef, text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
        let client = self.client().filter(|c| c.ready()).cloned();
        let target = self
            .docs
            .get(&doc.key)
            .and_then(|d| d.uri.clone().filter(|_| d.opened));
        let (Some(client), Some(uri)) = (client, target) else {
            return Oneshot::ready(Err(tr!(
                "javascript.formatting_javascript_typescript_needs_language"
            )
            .into()));
        };
        let tabs = text.contains("\n\t");
        let size = text
            .lines()
            .map(|l| l.len() - l.trim_start_matches(' ').len())
            .find(|&n| n > 0)
            .unwrap_or(2);
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": uri}, "options": {"tabSize": size, "insertSpaces": !tabs}}),
            move |result| {
                slot.fulfill(match result {
                    Ok(Value::Array(edits)) => {
                        let converted: Vec<TextEdit> = edits
                            .iter()
                            .filter_map(|e| {
                                let range = &e["range"];
                                let start = from_position(&snapshot, range["start"]["line"].as_u64()? as usize, range["start"]["character"].as_u64()? as usize);
                                let end = from_position(&snapshot, range["end"]["line"].as_u64()? as usize, range["end"]["character"].as_u64()? as usize);
                                Some(TextEdit {
                                    range: start..end.max(start),
                                    new_text: e["newText"].as_str()?.to_string(),
                                })
                            })
                            .collect();
                        let new = apply_edits(&snapshot, &converted);
                        Ok(edits_between(&snapshot, &new))
                    }
                    Ok(_) => Ok(Vec::new()),
                    Err(e) => Err(e["message"].as_str().unwrap_or_default().to_string()),
                });
            },
        );
        answer
    }

    /// The "organize imports" action of tsserver: sorted imports, the unused ones removed.
    fn organize_imports(
        &mut self,
        doc: &DocRef,
        text: &str,
    ) -> Oneshot<Result<Vec<TextEdit>, String>> {
        let client = self.client().filter(|c| c.ready()).cloned();
        let target = self
            .docs
            .get(&doc.key)
            .and_then(|d| d.uri.clone().filter(|_| d.opened));
        let (Some(client), Some(uri)) = (client, target) else {
            return Oneshot::ready(Ok(Vec::new()));
        };
        let end = to_position(text, text.len());
        let params = json!({
            "textDocument": {"uri": uri},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": end.0, "character": end.1}},
            "context": {"diagnostics": [], "only": ["source.organizeImports"], "triggerKind": 1},
        });
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request("textDocument/codeAction", params, move |result| {
            let edits = result
                .map(|r| fixes_from_server(&r, &uri, &snapshot))
                .unwrap_or_default()
                .into_iter()
                .next()
                .map(|fix| fix.edits)
                .unwrap_or_default();
            slot.fulfill(Ok(edits));
        });
        answer
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
            return tr!("javascript.installing_js_server").into();
        }
        match &self.server {
            Server::NotStarted => String::new(),
            Server::Running(client) if client.ready() => "tsserver".into(),
            Server::Running(_) => "tsserver…".into(),
            Server::Missing => tr!("javascript.js_local_analysis").into(),
            Server::Failed(reason) => trf!(
                "javascript.js_server_failed",
                reason = reason.chars().take(60).collect::<String>()
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
        tr!("javascript.completion_tooltips_syntax_errors_javascript")
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
                tr!("javascript.javascript_install_language_server_typescript"),
            ),
            (
                "js.restart",
                tr!("javascript.javascript_restart_language_server"),
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
                    host.balloon(
                        "JavaScript",
                        tr!("javascript.node_js_cannot_found_install").into(),
                        true,
                    );
                    return;
                }
                host.notify(tr!("javascript.installing_javascript_language_server").into());
                server::control(|c| c.install_requested = true);
            }
            "js.restart" => server::control(|c| c.restart_requested = true),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/mod/tests.rs"]
mod tests;
