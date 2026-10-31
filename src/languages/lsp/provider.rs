//! A [`Provider`] for a language that has a language server, with the analysis of the IDE as a
//! fallback: what the server answers when it runs, what the [`Language`] knows by itself
//! (tree-sitter) before it runs, or without it. Rust and Java are made of it; a language only
//! says where its server is and what it knows by itself.

use crate::languages::{
    Completion, CompletionItem, Diagnostic, DocRef, FileEdit, Hover, Location, Oneshot, Provider,
    QuickFix, Severity, Symbol, TextEdit,
    format::{apply, edits_between},
    lsp::{
        self, Client, Launch,
        convert::{
            completion_from_server, fixes_from_server, locations_from_server, markup_text, plain,
            severity, split_hover, symbols_from_server, workspace_edit,
        },
        from_position, to_position,
    },
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// What the commands of a language's plugin ask of its provider, and what the provider tells
/// back. Each language keeps one in a `static`.
#[derive(Default)]
pub struct Control {
    pub restart_requested: bool,
    pub installing: bool,
    /// Files saved since the provider last looked: the server checks the code on a save.
    pub saved: Vec<PathBuf>,
    /// Messages for the user: (text, is an error).
    pub messages: Vec<(String, bool)>,
}

impl Control {
    pub const fn new() -> Self {
        Self {
            restart_requested: false,
            installing: false,
            saved: Vec::new(),
            messages: Vec::new(),
        }
    }
}

/// Runs `f` on a control, whatever a panicking thread left in it.
pub fn with_control<R>(control: &Mutex<Control>, f: impl FnOnce(&mut Control) -> R) -> R {
    let mut guard = control.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// What a language brings to [`LspProvider`].
pub trait Language: Send + Sync + 'static {
    /// Whether it handles a language as `project::language` names it (`rs`, `java`).
    fn handles(&self, language: &str) -> bool;
    /// Its name for the user (`Rust`).
    fn name(&self) -> &'static str;
    /// The `languageId` the protocol wants (`rust`).
    fn language_id(&self) -> &'static str;
    /// The `languageId` of a document of `language` (as `project::language` names it), for a
    /// server of several languages (CSS, SCSS, Less).
    fn language_id_for(&self, language: &str) -> &'static str {
        let _ = language;
        self.language_id()
    }
    /// Its own formatter is used even when the server runs.
    fn formats_itself(&self) -> bool {
        false
    }
    /// Whether the server reads documents of `language`; the others get the local analysis.
    fn server_reads(&self, language: &str) -> bool {
        let _ = language;
        true
    }
    /// The name of its server, for the status bar (`rust-analyzer`).
    fn server_name(&self) -> &'static str;
    /// How to start the server for a project; None when it is not installed. Called on a
    /// thread of its own: it may run programs.
    fn find_server(&self, root: &Path) -> Option<Launch>;
    fn control(&self) -> &'static Mutex<Control>;
    /// Why an action needs the server, and how to install it.
    fn needs_server(&self) -> String;
    /// The status while the server is being installed.
    fn installing(&self) -> &'static str;
    fn syntax_errors(&self, text: &str) -> Vec<Diagnostic>;
    fn symbols(&self, text: &str) -> Vec<Symbol>;
    fn highlights(&self, text: &str, offset: usize) -> Vec<Range<usize>>;
    fn definition(&self, text: &str, offset: usize) -> Vec<Location>;
    fn complete(&self, text: &str, offset: usize, explicit: bool) -> Option<Completion>;
    fn word_start(&self, text: &str, offset: usize) -> usize {
        let mut from = offset;
        for (i, c) in text[..offset].char_indices().rev() {
            if !(c.is_alphanumeric() || c == '_') {
                break;
            }
            from = i;
        }
        from
    }
    /// The characters after which the server is asked for members (`.`, `:`).
    fn triggers(&self) -> &'static [char] {
        &['.']
    }
    /// The text formatted without the server, by a formatter of the language (rustfmt). Runs
    /// on a thread of its own.
    fn format_local(&self, _text: &str, _path: Option<&Path>) -> Result<String, String> {
        Err(self.needs_server())
    }
    /// The spaces of an indentation, for the server's formatter.
    fn tab_size(&self) -> usize {
        4
    }
}

enum Server {
    NotStarted,
    /// The language is asked where the server is, in the background.
    Looking(Oneshot<Option<Launch>>),
    Missing,
    Running(Arc<Client>),
    Failed(String),
}

struct DocData {
    path: Option<PathBuf>,
    /// The language of the document, as `project::language` names it.
    language: String,
    uri: Option<String>,
    text: String,
    version: u64,
    /// The server knows the document.
    opened: bool,
    /// What the diagnostics were computed for: the text version and the server's generation.
    analyzed: Option<(u64, u64)>,
    diagnostics: Arc<Vec<Diagnostic>>,
}

pub struct LspProvider<L: Language> {
    lang: Arc<L>,
    root: PathBuf,
    repaint: Arc<dyn Fn() + Send + Sync>,
    docs: HashMap<u64, DocData>,
    server: Server,
}

/// The byte range of a protocol range in `text`.
pub fn range_of(range: &Value, text: &str) -> Option<Range<usize>> {
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
    Some(start..end.max(start))
}

impl<L: Language> LspProvider<L> {
    pub fn new(lang: L, root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            lang: Arc::new(lang),
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

    /// The server, ready, and the address of a document it knows.
    pub(crate) fn target(&self, key: u64) -> Option<(Arc<Client>, String)> {
        let client = self.client().filter(|c| c.ready())?.clone();
        let doc = self.docs.get(&key)?;
        Some((client, doc.uri.clone().filter(|_| doc.opened)?))
    }

    /// Executes what the plugin's commands asked for, and tells the server about saved files.
    fn poll_control(&mut self) {
        let (restart, saved) = with_control(self.lang.control(), |c| {
            (
                std::mem::take(&mut c.restart_requested),
                std::mem::take(&mut c.saved),
            )
        });
        if restart {
            self.server = Server::NotStarted;
            for doc in self.docs.values_mut() {
                doc.opened = false;
                doc.analyzed = None;
            }
        }
        if let Some(client) = self.client().cloned() {
            for path in saved {
                let known = self
                    .docs
                    .values()
                    .find(|d| d.opened && d.path.as_deref() == Some(path.as_path()));
                if let Some(uri) = known.and_then(|d| d.uri.clone()) {
                    client.notify(
                        "textDocument/didSave",
                        json!({"textDocument": {"uri": uri}}),
                    );
                }
            }
        }
    }

    fn ensure_server(&mut self) {
        match &self.server {
            Server::NotStarted => {
                if crate::wsl::parse(&self.root).is_some() {
                    self.server = Server::Missing;
                    return;
                }
                let slot = Oneshot::pending();
                let answer = slot.clone();
                let (lang, root, repaint) =
                    (self.lang.clone(), self.root.clone(), self.repaint.clone());
                std::thread::spawn(move || {
                    answer.fulfill(lang.find_server(&root));
                    repaint();
                });
                self.server = Server::Looking(slot);
            }
            Server::Looking(slot) => {
                let Some(found) = slot.take() else { return };
                self.server = match found {
                    None => Server::Missing,
                    Some(launch) => {
                        match Client::start(&launch, &self.root, self.repaint.clone()) {
                            Ok(client) => Server::Running(client),
                            Err(e) => Server::Failed(format!("{e:#}")),
                        }
                    }
                };
                // The documents opened while looking are told now.
                let keys: Vec<u64> = self.docs.keys().copied().collect();
                for key in keys {
                    self.sync(key);
                }
            }
            Server::Running(client) => {
                let failure = client.failure().or_else(|| {
                    client.exited().then(|| {
                        let log = client.log();
                        match log.lines().last() {
                            Some(last) if !last.is_empty() => last.to_string(),
                            _ => tr!("lsp.server_stopped_lower").to_string(),
                        }
                    })
                });
                if let Some(message) = failure {
                    let text = trf!(
                        "lsp.server_failed_message",
                        server = self.lang.server_name(),
                        message = message
                    );
                    with_control(self.lang.control(), |c| c.messages.push((text, true)));
                    self.server = Server::Failed(message);
                }
            }
            Server::Missing | Server::Failed(_) => {}
        }
    }

    /// Tells the server about the current text of a document.
    fn sync(&mut self, key: u64) {
        let Some(client) = self.client().cloned() else {
            return;
        };
        let Some(doc) = self.docs.get_mut(&key) else {
            return;
        };
        let language_id = self.lang.language_id_for(&doc.language);
        let Some(uri) = &doc.uri else {
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
                json!({"textDocument": {"uri": uri, "languageId": language_id, "version": doc.version, "text": doc.text}}),
            );
            doc.opened = true;
        }
    }

    /// Asks the server for places, the local analysis answering without it or when it knows
    /// nothing.
    fn locate(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        method: &str,
        extra: Value,
        local: Vec<Location>,
    ) -> Oneshot<Vec<Location>> {
        let Some((client, uri)) = self.target(doc.key) else {
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

impl<L: Language> Provider for LspProvider<L> {
    fn handles(&self, language: &str) -> bool {
        self.lang.handles(language)
    }

    fn update(&mut self, doc: &DocRef, text: &str, version: u64) {
        self.poll_control();
        self.ensure_server();
        let uri = doc
            .path
            .as_ref()
            .filter(|p| crate::wsl::parse(p).is_none())
            .filter(|_| self.lang.server_reads(&doc.language))
            .map(|p| lsp::path_to_uri(p));
        let client = self.client().cloned();
        let data = self.docs.entry(doc.key).or_insert_with(|| DocData {
            path: doc.path.clone(),
            language: doc.language.clone(),
            uri: None,
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
        if matches!(self.server, Server::Looking(_) | Server::Running(_)) {
            self.ensure_server();
        }
        let generation = self.client().map_or(0, |c| c.generation());
        let client = self.client().cloned();
        let lang = self.lang.clone();
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
                        lang.server_name().into()
                    } else {
                        d.source
                    },
                });
            }
        }
        for native in lang.syntax_errors(&data.text) {
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
        let native = self.lang.complete(text, offset, explicit);
        let Some((client, uri)) = self.target(doc.key) else {
            return Oneshot::ready(native);
        };
        let offset = offset.min(text.len());
        let from = self.lang.word_start(text, offset);
        let typed = &text[from..offset];
        let triggers = self.lang.triggers();
        // After a trigger only the server knows the members.
        if typed.is_empty() && !explicit && !text[..from].ends_with(triggers) {
            return Oneshot::ready(None);
        }
        let (line, character) = to_position(text, offset);
        let trigger = text[..from]
            .chars()
            .next_back()
            .filter(|c| triggers.contains(c));
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
                let from_server = result
                    .ok()
                    .and_then(|r| completion_from_server(&r, &snapshot, from));
                slot.fulfill(from_server.or(native));
            },
        );
        answer
    }

    fn hover(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Option<Hover>> {
        let Some((client, uri)) = self.target(doc.key) else {
            return Oneshot::ready(None);
        };
        let (line, character) = to_position(text, offset);
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        client.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
            move |result| {
                let hover = result.ok().and_then(|r| {
                    let markdown = markup_text(&r["contents"]);
                    if markdown.trim().is_empty() {
                        return None;
                    }
                    let (title, body) = split_hover(&markdown);
                    let range = range_of(&r["range"], &snapshot).unwrap_or(offset..offset);
                    Some(Hover { range, title, body })
                });
                slot.fulfill(hover);
            },
        );
        answer
    }

    fn definition(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        let local = self.lang.definition(text, offset);
        self.locate(
            doc,
            text,
            offset,
            "textDocument/definition",
            json!({}),
            local,
        )
    }

    fn type_definition(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        let method = "textDocument/typeDefinition";
        self.locate(doc, text, offset, method, json!({}), Vec::new())
    }

    fn implementation(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        let method = "textDocument/implementation";
        self.locate(doc, text, offset, method, json!({}), Vec::new())
    }

    fn references(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        let local: Vec<Location> = self
            .lang
            .highlights(text, offset)
            .into_iter()
            .map(|range| Location::in_text(None, text, range))
            .collect();
        let extra = json!({"context": {"includeDeclaration": false}});
        self.locate(doc, text, offset, "textDocument/references", extra, local)
    }

    fn highlights(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Range<usize>>> {
        let local = self.lang.highlights(text, offset);
        let Some((client, uri)) = self.target(doc.key) else {
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
                let places: Vec<Range<usize>> = result
                    .ok()
                    .and_then(|r| r.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|h| range_of(&h["range"], &snapshot))
                    .collect();
                // The server answers nothing for a keyword or a literal: so be it.
                slot.fulfill(places);
            },
        );
        answer
    }

    fn symbols(&mut self, doc: &DocRef, text: &str) -> Oneshot<Vec<Symbol>> {
        let local = self.lang.symbols(text);
        let Some((client, uri)) = self.target(doc.key) else {
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

    fn rename(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        new_name: &str,
    ) -> Oneshot<Result<Vec<FileEdit>, String>> {
        let Some((client, uri)) = self.target(doc.key) else {
            return Oneshot::ready(Err(self.lang.needs_server()));
        };
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        let repaint = self.repaint.clone();
        let (line, character) = to_position(text, offset);
        let params = json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}, "newName": new_name});
        client.request("textDocument/rename", params, move |result| {
            slot.fulfill(match result {
                Ok(r) => {
                    let plan = workspace_edit(&r, &uri, &snapshot);
                    if plan.is_empty() {
                        Err(tr!("lsp.nothing_to_rename").into())
                    } else {
                        Ok(plan)
                    }
                }
                Err(e) => Err(e["message"]
                    .as_str()
                    .unwrap_or(tr!("lsp.nothing_to_rename"))
                    .to_string()),
            });
            repaint();
        });
        answer
    }

    fn quick_fixes(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<QuickFix>> {
        let Some((client, uri)) = self.target(doc.key) else {
            return Oneshot::ready(Vec::new());
        };
        let (found, _) = client.diagnostics(&lsp::normalize_uri(&uri));
        let here = to_position(text, offset);
        let diagnostics: Vec<Value> = found
            .into_iter()
            .filter(|d| d.start <= here && here <= d.end)
            .map(|d| d.raw)
            .collect();
        let params = json!({
            "textDocument": {"uri": uri},
            "range": {"start": {"line": here.0, "character": here.1}, "end": {"line": here.0, "character": here.1}},
            "context": {"diagnostics": diagnostics, "triggerKind": 1},
        });
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        let repaint = self.repaint.clone();
        client.request("textDocument/codeAction", params, move |result| {
            let fixes = result
                .map(|r| fixes_from_server(&r, &uri, &snapshot))
                .unwrap_or_default();
            slot.fulfill(fixes);
            repaint();
        });
        answer
    }

    /// The server's formatter; without it, or when it cannot, the language's own.
    fn format(&mut self, doc: &DocRef, text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
        let answer = Oneshot::pending();
        let slot = answer.clone();
        let snapshot = text.to_string();
        let path = doc.path.clone();
        let (lang, repaint) = (self.lang.clone(), self.repaint.clone());
        let local = move |slot: Oneshot<Result<Vec<TextEdit>, String>>| {
            std::thread::spawn(move || {
                let result = lang
                    .format_local(&snapshot, path.as_deref())
                    .map(|new| edits_between(&snapshot, &new));
                slot.fulfill(result);
                repaint();
            });
        };
        let Some((client, uri)) = self.target(doc.key).filter(|_| !self.lang.formats_itself())
        else {
            local(slot);
            return answer;
        };
        let server_text = text.to_string();
        let tab_size = self.lang.tab_size();
        client.request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": uri}, "options": {"tabSize": tab_size, "insertSpaces": true}}),
            move |result| match result {
                Ok(Value::Array(edits)) => {
                    let converted: Vec<TextEdit> = edits
                        .iter()
                        .filter_map(|e| {
                            Some(TextEdit {
                                range: range_of(&e["range"], &server_text)?,
                                new_text: e["newText"].as_str()?.to_string(),
                            })
                        })
                        .collect();
                    // One edit for the whole file: the smallest ones are better for the caret.
                    let new = apply(&server_text, &converted);
                    slot.fulfill(Ok(edits_between(&server_text, &new)));
                }
                Ok(_) => slot.fulfill(Ok(Vec::new())),
                Err(_) => local(slot),
            },
        );
        answer
    }

    /// The "organize imports" action of the server, when it has one.
    fn organize_imports(
        &mut self,
        doc: &DocRef,
        text: &str,
    ) -> Oneshot<Result<Vec<TextEdit>, String>> {
        let Some((client, uri)) = self.target(doc.key) else {
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

    fn lsp_target(&mut self, doc: &DocRef) -> Option<(Arc<Client>, String)> {
        self.target(doc.key)
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

    fn status(&self) -> String {
        if with_control(self.lang.control(), |c| c.installing) {
            return self.lang.installing().into();
        }
        let name = self.lang.server_name();
        match &self.server {
            Server::NotStarted | Server::Looking(_) => String::new(),
            Server::Running(client) if client.ready() => name.into(),
            Server::Running(_) => format!("{name}…"),
            Server::Missing => trf!("lsp.local_analysis", language = self.lang.name()),
            Server::Failed(reason) => trf!(
                "lsp.server_failed_status",
                language = self.lang.name(),
                reason = reason.chars().take(60).collect::<String>()
            ),
        }
    }
}
