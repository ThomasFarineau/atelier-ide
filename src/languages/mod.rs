//! Language intelligence: diagnostics, completion and hover for the code in the editor.
//!
//! Each language brings a [`Provider`]. The [`Service`] is the hub the editor talks to: it
//! routes a document to the provider of its language and hands back results, either at once
//! (analysis done in Rust) or later (a language server answering in the background), through
//! a [`Oneshot`] the editor polls.

pub mod css;
pub mod format;
pub mod html;
pub mod inspect;
pub mod java;
pub mod javascript;
pub mod lsp;
pub mod python;
pub mod rust;
pub mod syntax;
pub mod templates;
pub mod typescript;

use std::{
    borrow::Cow,
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A problem found in the code, as a byte range of the document.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    /// Where it comes from: "syntaxe", "tsserver", "rust-analyzer"…
    pub source: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Variable,
    Constant,
    Function,
    Method,
    Property,
    Class,
    Keyword,
    Module,
    Snippet,
    File,
    Folder,
    Parameter,
    Interface,
    Enum,
    Text,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CompletionItem {
    /// What the list shows and the typed prefix is matched against.
    pub label: String,
    pub insert: String,
    pub kind: ItemKind,
    /// Type or signature, shown at the right of the label.
    pub detail: Option<String>,
    pub doc: Option<String>,
    /// Caret position inside `insert` once inserted; the end when None.
    pub caret: Option<usize>,
    /// Lower comes first among equal matches.
    pub rank: u32,
    /// Where the text that the item replaces starts, when it is before the word typed (a
    /// postfix template takes the expression before the dot). The word start when None.
    pub start: Option<usize>,
    /// What the provider needs to complete the item later (documentation, signature).
    pub raw: Option<Arc<String>>,
}

impl CompletionItem {
    pub fn new(label: impl Into<String>, kind: ItemKind) -> Self {
        let label = label.into();
        Self {
            insert: label.clone(),
            label,
            kind,
            detail: None,
            doc: None,
            caret: None,
            rank: 50,
            start: None,
            raw: None,
        }
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }
    pub fn rank(mut self, rank: u32) -> Self {
        self.rank = rank;
        self
    }
    pub fn start(mut self, start: usize) -> Self {
        self.start = Some(start);
        self
    }
    pub fn insert(mut self, insert: impl Into<String>, caret: Option<usize>) -> Self {
        self.insert = insert.into();
        self.caret = caret;
        self
    }
}

/// Candidates for a position. `from` is where the word being completed starts: the editor
/// filters the items by what is typed between `from` and the caret.
#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub from: usize,
    pub items: Vec<CompletionItem>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hover {
    pub range: Range<usize>,
    /// The declaration or signature, shown as code.
    pub title: String,
    /// Documentation, plain text.
    pub body: String,
}

/// A change of the text: the bytes of `range` are replaced by `new_text`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub new_text: String,
}

/// The changes of one file: the document the request was made in (path None) or another.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileEdit {
    pub path: Option<PathBuf>,
    pub edits: Vec<TextEdit>,
}

/// An action offered on the code at the caret (Alt+Enter): add an import, and so on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuickFix {
    pub title: String,
    pub edits: Vec<TextEdit>,
}

/// A named thing of a document, for the structure of a file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: ItemKind,
    /// Parameters or type, shown after the name.
    pub detail: String,
    /// The bytes of the name.
    pub range: Range<usize>,
    /// How deep it is nested: a method of a class is one level down.
    pub depth: usize,
}

/// A place in a document: where a name is declared, or where it is used.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Location {
    /// The file, None for the document the request was made in.
    pub path: Option<PathBuf>,
    /// 0-based line and UTF-16 column, as the protocol counts.
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    /// The line of code, to tell several places apart.
    pub preview: String,
}

impl Location {
    /// The place of a byte range of `text`.
    pub fn in_text(path: Option<PathBuf>, text: &str, range: Range<usize>) -> Self {
        let (line, column) = lsp::to_position(text, range.start);
        let (end_line, end_column) = lsp::to_position(text, range.end);
        let start = text[..range.start.min(text.len())]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        Self {
            path,
            line,
            column,
            end_line,
            end_column,
            preview: text[start..end].trim().chars().take(120).collect(),
        }
    }

    /// The byte range of the place in `text`.
    pub fn range_in(&self, text: &str) -> Range<usize> {
        let start = lsp::from_position(text, self.line, self.column);
        let end = lsp::from_position(text, self.end_line, self.end_column).max(start);
        start..end
    }
}

/// A result that arrives later; the editor polls it each frame.
pub struct Oneshot<T>(Arc<Mutex<Option<T>>>);

impl<T> Clone for Oneshot<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Oneshot<T> {
    pub fn pending() -> Self {
        Self(Arc::default())
    }
    pub fn ready(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }
    pub fn fulfill(&self, value: T) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(value);
        }
    }
    /// The value, once; None while it is not there yet.
    pub fn take(&self) -> Option<T> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// What a language offers for the documents it handles.
pub trait Provider: Send {
    fn handles(&self, language: &str) -> bool;
    /// The document changed (or is seen for the first time). `version` grows with each edit.
    fn update(&mut self, doc: &DocRef, text: &str, version: u64);
    fn close(&mut self, key: u64);
    fn diagnostics(&mut self, key: u64) -> Arc<Vec<Diagnostic>>;
    fn complete(
        &mut self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        explicit: bool,
    ) -> Oneshot<Option<Completion>>;
    fn hover(&mut self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Option<Hover>>;
    /// Where the name at `offset` is declared.
    fn definition(&mut self, _doc: &DocRef, _text: &str, _offset: usize) -> Oneshot<Vec<Location>> {
        Oneshot::ready(Vec::new())
    }
    /// Where the name at `offset` is used.
    fn references(&mut self, _doc: &DocRef, _text: &str, _offset: usize) -> Oneshot<Vec<Location>> {
        Oneshot::ready(Vec::new())
    }
    /// Where the type of the name at `offset` is declared.
    fn type_definition(
        &mut self,
        _doc: &DocRef,
        _text: &str,
        _offset: usize,
    ) -> Oneshot<Vec<Location>> {
        Oneshot::ready(Vec::new())
    }
    /// The implementations of the name at `offset`.
    fn implementation(
        &mut self,
        _doc: &DocRef,
        _text: &str,
        _offset: usize,
    ) -> Oneshot<Vec<Location>> {
        Oneshot::ready(Vec::new())
    }
    /// Every place in the document that is the same name as the one at `offset`, the
    /// declaration included (for the highlight under the caret).
    fn highlights(
        &mut self,
        _doc: &DocRef,
        _text: &str,
        _offset: usize,
    ) -> Oneshot<Vec<Range<usize>>> {
        Oneshot::ready(Vec::new())
    }
    /// The functions, classes and constants of the document.
    fn symbols(&mut self, _doc: &DocRef, _text: &str) -> Oneshot<Vec<Symbol>> {
        Oneshot::ready(Vec::new())
    }
    /// The changes that rename the name at `offset`, or why it cannot be.
    fn rename(
        &mut self,
        _doc: &DocRef,
        _text: &str,
        _offset: usize,
        _new_name: &str,
    ) -> Oneshot<Result<Vec<FileEdit>, String>> {
        Oneshot::ready(Err(tr!("rename_not_supported_language").into()))
    }
    /// The actions offered at `offset`.
    fn quick_fixes(
        &mut self,
        _doc: &DocRef,
        _text: &str,
        _offset: usize,
    ) -> Oneshot<Vec<QuickFix>> {
        Oneshot::ready(Vec::new())
    }
    /// The edits that reformat the document (Reformat Code, Ctrl+Alt+L).
    fn format(&mut self, _doc: &DocRef, _text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
        Oneshot::ready(Err(tr!("no_formatter_language").into()))
    }
    /// The edits that sort the imports and remove the unused ones (Optimize Imports). None
    /// when the language cannot.
    fn organize_imports(
        &mut self,
        _doc: &DocRef,
        _text: &str,
    ) -> Oneshot<Result<Vec<TextEdit>, String>> {
        Oneshot::ready(Ok(Vec::new()))
    }
    /// The detail and documentation of an item that came without them.
    fn resolve(
        &mut self,
        _doc: &DocRef,
        _item: &CompletionItem,
    ) -> Oneshot<Option<CompletionItem>> {
        Oneshot::ready(None)
    }
    /// A line for the status bar: which engine answers.
    fn status(&self) -> String {
        String::new()
    }
    /// The language server and the address of the document when it is running and knows the
    /// document: signature help, inlay hints, refactorings and hierarchies go through it.
    fn lsp_target(&mut self, _doc: &DocRef) -> Option<(Arc<lsp::Client>, String)> {
        None
    }
    /// The problems the language server found in the files of the project, open or not (a Rust
    /// server checks the whole project, a Java one too).
    fn workspace_diagnostics(&mut self) -> Vec<(PathBuf, Vec<lsp::ServerDiagnostic>)> {
        Vec::new()
    }
}

/// How the providers identify a document.
#[derive(Clone, Debug)]
pub struct DocRef {
    pub key: u64,
    pub path: Option<PathBuf>,
    pub language: String,
}

#[derive(Default)]
struct Inner {
    providers: Vec<Box<dyn Provider>>,
    /// Provider of each open document.
    owners: HashMap<u64, usize>,
    versions: HashMap<u64, u64>,
    /// The code of other languages in each page (HTML, Pug), and the documents of those
    /// languages made from it (see `html::embedded`).
    pages: HashMap<u64, Page>,
    /// Documents made to format one block of code of a page, closed a while later.
    scratch: Vec<(u64, Instant)>,
    /// The document that answered the last completion of a page, for `resolve`.
    completed_in: HashMap<u64, DocRef>,
    /// The problems of each page with the ones of its code, and what they were made of.
    merged: HashMap<u64, (Vec<usize>, Arc<Vec<Diagnostic>>)>,
}

/// The code of other languages in a page.
#[derive(Default)]
struct Page {
    regions: Vec<html::embedded::Region>,
    docs: Vec<DocRef>,
}

/// The hub of language intelligence, shared by every editor.
#[derive(Default)]
pub struct Service {
    inner: Mutex<Inner>,
    /// Tells the interface that a result arrived in the background.
    repaint: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// The key of the document of `language` made from the page `key`.
fn embedded_key(key: u64, language: &str, salt: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ("embedded", key, language, salt).hash(&mut hasher);
    hasher.finish()
}

/// The document of `language` made from a page: `index.html` gives `index.html.js`.
fn embedded_doc(page: &DocRef, language: &'static str, key: u64, tag: &str) -> DocRef {
    DocRef {
        key,
        path: page.path.as_ref().map(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            p.with_file_name(format!("{name}{tag}.{language}"))
        }),
        language: language.to_string(),
    }
}

/// Places in the document of a page's code are places in the page.
fn page_locations(mut found: Vec<Location>, virtual_path: &Option<PathBuf>) -> Vec<Location> {
    for location in &mut found {
        if location.path.is_some() && &location.path == virtual_path {
            location.path = None;
        }
    }
    found
}

/// Waits in the background for every part, then gives them all at once.
fn join<T: Send + 'static, R: Send + 'static>(
    parts: Vec<Oneshot<T>>,
    repaint: Option<Arc<dyn Fn() + Send + Sync>>,
    finish: impl FnOnce(Vec<Option<T>>) -> R + Send + 'static,
) -> Oneshot<R> {
    let answer = Oneshot::pending();
    let slot = answer.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut got: Vec<Option<T>> = parts.iter().map(|_| None).collect();
        while got.iter().any(Option::is_none) && started.elapsed() < Duration::from_secs(10) {
            for (part, value) in parts.iter().zip(got.iter_mut()) {
                if value.is_none() {
                    *value = part.take();
                }
            }
            if got.iter().any(Option::is_none) {
                std::thread::sleep(Duration::from_millis(15));
            }
        }
        slot.fulfill(finish(got));
        if let Some(repaint) = repaint {
            repaint();
        }
    });
    answer
}

impl Service {
    /// The service with the providers of the supported languages, for a project folder.
    pub fn new(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let service = Self {
            repaint: Some(repaint.clone()),
            ..Self::default()
        };
        service.add(Box::new(javascript::JavaScript::new(root, repaint.clone())));
        service.add(Box::new(rust::provider(root, repaint.clone())));
        service.add(Box::new(java::provider(root, repaint.clone())));
        service.add(Box::new(css::provider(root, repaint.clone())));
        service.add(Box::new(html::provider(root, repaint.clone())));
        service.add(Box::new(html::pug_provider(root, repaint.clone())));
        service.add(Box::new(python::provider(root, repaint)));
        service
    }

    pub fn add(&self, provider: Box<dyn Provider>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.providers.push(provider);
        }
    }

    fn with<R>(&self, language: &str, f: impl FnOnce(&mut dyn Provider, usize) -> R) -> Option<R> {
        let mut inner = self.inner.lock().ok()?;
        let index = inner.providers.iter().position(|p| p.handles(language))?;
        Some(f(inner.providers[index].as_mut(), index))
    }

    pub fn handles(&self, language: &str) -> bool {
        self.inner
            .lock()
            .is_ok_and(|inner| inner.providers.iter().any(|p| p.handles(language)))
    }

    /// Tells the provider the document is at `version`. Cheap when nothing changed. A page
    /// also gives the code of other languages it holds to their providers.
    pub fn update(&self, doc: &DocRef, text: &str, version: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if !Self::update_in(&mut inner, doc, text, version) {
            return;
        }
        if html::is_markup(&doc.language) {
            let regions = html::embedded::regions(text, &doc.language);
            let docs: Vec<DocRef> = html::embedded::languages(&regions)
                .into_iter()
                .filter(|l| inner.providers.iter().any(|p| p.handles(l)))
                .map(|l| embedded_doc(doc, l, embedded_key(doc.key, l, 0), ""))
                .collect();
            for embedded in &docs {
                let code = html::embedded::document(text, &regions, &embedded.language);
                Self::update_in(&mut inner, embedded, &code, version);
            }
            let old = inner.pages.insert(
                doc.key,
                Page {
                    regions,
                    docs: docs.clone(),
                },
            );
            for gone in old
                .map(|p| p.docs)
                .unwrap_or_default()
                .into_iter()
                .filter(|o| !docs.iter().any(|d| d.key == o.key))
            {
                Self::close_in(&mut inner, gone.key);
            }
        }
        // The documents made to format code are closed once the formatting is long done.
        let expired: Vec<u64> = inner
            .scratch
            .iter()
            .filter(|(_, made)| made.elapsed() > Duration::from_secs(60))
            .map(|(key, _)| *key)
            .collect();
        if !expired.is_empty() {
            inner.scratch.retain(|(key, _)| !expired.contains(key));
            for key in expired {
                Self::close_in(&mut inner, key);
            }
        }
    }

    /// Tells the provider of `doc` its text. False when nothing changed, or no provider
    /// handles it.
    fn update_in(inner: &mut Inner, doc: &DocRef, text: &str, version: u64) -> bool {
        let Some(index) = inner
            .providers
            .iter()
            .position(|p| p.handles(&doc.language))
        else {
            return false;
        };
        if inner.versions.get(&doc.key) == Some(&version)
            && inner.owners.get(&doc.key) == Some(&index)
        {
            return false;
        }
        // A document whose language changed (saved under another name) leaves its old provider.
        if let Some(old) = inner.owners.insert(doc.key, index)
            && old != index
        {
            inner.providers[old].close(doc.key);
        }
        inner.versions.insert(doc.key, version);
        inner.providers[index].update(doc, text, version);
        true
    }

    fn close_in(inner: &mut Inner, key: u64) {
        if let Some(index) = inner.owners.remove(&key) {
            inner.providers[index].close(key);
        }
        inner.versions.remove(&key);
        inner.completed_in.remove(&key);
        inner.merged.remove(&key);
        if let Some(page) = inner.pages.remove(&key) {
            for doc in page.docs {
                Self::close_in(inner, doc.key);
            }
        }
    }

    pub fn close(&self, key: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        Self::close_in(&mut inner, key);
    }

    /// The problems of a document; for a page, with the ones its code has in its own language.
    pub fn diagnostics(&self, doc: &DocRef) -> Arc<Vec<Diagnostic>> {
        let Ok(mut inner) = self.inner.lock() else {
            return Arc::default();
        };
        let Some(index) = inner
            .providers
            .iter()
            .position(|p| p.handles(&doc.language))
        else {
            return Arc::default();
        };
        let own = inner.providers[index].diagnostics(doc.key);
        let Some(page) = inner.pages.get(&doc.key) else {
            return own;
        };
        let docs = page.docs.clone();
        let regions = page.regions.clone();
        let mut parts = vec![own];
        for embedded in &docs {
            let found = inner
                .providers
                .iter_mut()
                .find(|p| p.handles(&embedded.language))
                .map(|p| p.diagnostics(embedded.key))
                .unwrap_or_default();
            parts.push(found);
        }
        // Made of the same parts as last time: the same list.
        let made_of: Vec<usize> = parts.iter().map(|p| Arc::as_ptr(p) as usize).collect();
        if let Some((cached_of, cached)) = inner.merged.get(&doc.key)
            && *cached_of == made_of
        {
            return cached.clone();
        }
        let mut all: Vec<Diagnostic> = parts[0].as_ref().clone();
        for (embedded, found) in docs.iter().zip(&parts[1..]) {
            // Only what is about the code: not the spaces that stand for the rest of the page.
            all.extend(
                found
                    .iter()
                    .filter(|d| {
                        regions.iter().any(|r| {
                            r.analyzed
                                && r.language == embedded.language
                                && d.range.start + r.prefix.len() >= r.range.start
                                && d.range.start <= r.range.end + r.suffix.len()
                        })
                    })
                    .cloned(),
            );
        }
        all.sort_by_key(|d| (d.range.start, d.severity));
        let merged = Arc::new(all);
        inner.merged.insert(doc.key, (made_of, merged.clone()));
        merged
    }

    /// The document that answers at `offset` of `doc`, with its text: for the code of another
    /// language in a page, the document of that language made from the page.
    fn route<'a>(&self, doc: &DocRef, text: &'a str, offset: usize) -> (DocRef, Cow<'a, str>) {
        if !html::is_markup(&doc.language) {
            return (doc.clone(), Cow::Borrowed(text));
        }
        let regions = html::embedded::regions(text, &doc.language);
        let Some(region) = html::embedded::region_at(&regions, offset).filter(|r| r.analyzed)
        else {
            return (doc.clone(), Cow::Borrowed(text));
        };
        let language = region.language;
        if !self.handles(language) {
            return (doc.clone(), Cow::Borrowed(text));
        }
        let embedded = embedded_doc(doc, language, embedded_key(doc.key, language, 0), "");
        let code = html::embedded::document(text, &regions, language);
        (embedded, Cow::Owned(code))
    }

    pub fn complete(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        explicit: bool,
    ) -> Oneshot<Option<Completion>> {
        let (target, text) = self.route(doc, text, offset);
        if let Ok(mut inner) = self.inner.lock() {
            inner.completed_in.insert(doc.key, target.clone());
        }
        self.with(&target.language, |p, _| {
            p.complete(&target, &text, offset, explicit)
        })
        .unwrap_or_else(|| Oneshot::ready(None))
    }

    pub fn hover(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Option<Hover>> {
        let (target, text) = self.route(doc, text, offset);
        self.with(&target.language, |p, _| p.hover(&target, &text, offset))
            .unwrap_or_else(|| Oneshot::ready(None))
    }

    /// Places found by the provider that answers at `offset`, as places of `doc`.
    fn locate(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        find: impl FnOnce(&mut dyn Provider, &DocRef, &str) -> Oneshot<Vec<Location>>,
    ) -> Oneshot<Vec<Location>> {
        let (target, text) = self.route(doc, text, offset);
        let found = self
            .with(&target.language, |p, _| find(p, &target, &text))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()));
        if target.key == doc.key {
            return found;
        }
        let path = target.path.clone();
        join(vec![found], self.repaint.clone(), move |mut parts| {
            page_locations(parts.pop().flatten().unwrap_or_default(), &path)
        })
    }

    pub fn type_definition(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, |p, d, t| p.type_definition(d, t, offset))
    }

    pub fn implementation(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, |p, d, t| p.implementation(d, t, offset))
    }

    pub fn highlights(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Range<usize>>> {
        let (target, text) = self.route(doc, text, offset);
        self.with(&target.language, |p, _| {
            p.highlights(&target, &text, offset)
        })
        .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    /// The structure of a document; for a page, the one of its code is put under the element
    /// that holds it.
    pub fn symbols(&self, doc: &DocRef, text: &str) -> Oneshot<Vec<Symbol>> {
        let own = self
            .with(&doc.language, |p, _| p.symbols(doc, text))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()));
        if !html::is_markup(&doc.language) {
            return own;
        }
        let regions = html::embedded::regions(text, &doc.language);
        let mut parts = vec![own];
        for language in html::embedded::languages(&regions) {
            let embedded = embedded_doc(doc, language, embedded_key(doc.key, language, 0), "");
            let code = html::embedded::document(text, &regions, language);
            if let Some(found) = self.with(language, |p, _| p.symbols(&embedded, &code)) {
                parts.push(found);
            }
        }
        if parts.len() == 1 {
            return parts.pop().unwrap_or_else(|| Oneshot::ready(Vec::new()));
        }
        join(parts, self.repaint.clone(), |parts| {
            let mut parts = parts.into_iter().map(Option::unwrap_or_default);
            let mut all: Vec<Symbol> = parts.next().unwrap_or_default();
            let page = all.clone();
            for symbol in parts.flatten() {
                // One level under the element the code is in: the last one that starts
                // before it.
                let parent = page
                    .iter()
                    .rev()
                    .find(|s| s.range.start <= symbol.range.start)
                    .map_or(0, |s| s.depth + 1);
                all.push(Symbol {
                    depth: symbol.depth + parent,
                    ..symbol
                });
            }
            all.sort_by_key(|s| s.range.start);
            all
        })
    }

    pub fn rename(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        new_name: &str,
    ) -> Oneshot<Result<Vec<FileEdit>, String>> {
        let (target, text) = self.route(doc, text, offset);
        let answer = self
            .with(&target.language, |p, _| {
                p.rename(&target, &text, offset, new_name)
            })
            .unwrap_or_else(|| Oneshot::ready(Err(tr!("rename_not_supported_language").into())));
        if target.key == doc.key {
            return answer;
        }
        // The edits of the document of the page's code are edits of the page.
        let path = target.path.clone();
        join(vec![answer], self.repaint.clone(), move |mut parts| {
            parts
                .pop()
                .flatten()
                .unwrap_or_else(|| Err(tr!("rename_not_supported_language").into()))
                .map(|edits| {
                    edits
                        .into_iter()
                        .map(|mut edit| {
                            if edit.path.is_some() && edit.path == path {
                                edit.path = None;
                            }
                            edit
                        })
                        .collect()
                })
        })
    }

    pub fn quick_fixes(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<QuickFix>> {
        let (target, text) = self.route(doc, text, offset);
        self.with(&target.language, |p, _| {
            p.quick_fixes(&target, &text, offset)
        })
        .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    /// The problems the language servers found in the files of the project; not the ones of
    /// the documents made from the code of pages, which are not files.
    pub fn workspace_diagnostics(&self) -> Vec<(PathBuf, Vec<lsp::ServerDiagnostic>)> {
        let Ok(mut inner) = self.inner.lock() else {
            return Vec::new();
        };
        let made: Vec<PathBuf> = inner
            .pages
            .values()
            .flat_map(|p| p.docs.iter().filter_map(|d| d.path.clone()))
            .collect();
        inner
            .providers
            .iter_mut()
            .flat_map(|p| p.workspace_diagnostics())
            .filter(|(path, _)| {
                !made.contains(path) && !path.to_string_lossy().contains(".format-")
            })
            .collect()
    }

    /// The language server of a document, and the document's address, when it can be asked.
    pub fn lsp_target(&self, doc: &DocRef) -> Option<(Arc<lsp::Client>, String)> {
        self.with(&doc.language, |p, _| p.lsp_target(doc)).flatten()
    }

    /// The signature of the call at `offset`, with the parameter being typed (Ctrl+P).
    pub fn signature_help(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Option<lsp::extras::Signature>> {
        let (target, text) = self.route(doc, text, offset);
        match self.lsp_target(&target) {
            Some((client, uri)) => lsp::extras::signature_help(&client, &uri, &text, offset),
            None => Oneshot::ready(None),
        }
    }

    /// The hints (types, argument names) of the lines `first..=last`.
    pub fn inlay_hints(
        &self,
        doc: &DocRef,
        text: &str,
        first: usize,
        last: usize,
    ) -> Oneshot<Vec<lsp::extras::InlayHint>> {
        match self.lsp_target(doc) {
            Some((client, uri)) => lsp::extras::inlay_hints(&client, &uri, text, first, last),
            None => Oneshot::ready(Vec::new()),
        }
    }

    /// The refactorings the server offers for a range of the document.
    pub fn refactorings(
        &self,
        doc: &DocRef,
        text: &str,
        range: Range<usize>,
    ) -> Oneshot<Vec<lsp::extras::Refactoring>> {
        let (target, text) = self.route(doc, text, range.start);
        match self.lsp_target(&target) {
            Some((client, uri)) => lsp::extras::refactorings(&client, &uri, &text, range),
            None => Oneshot::ready(Vec::new()),
        }
    }

    /// The type or the function at `offset`, to start a hierarchy from.
    pub fn prepare_hierarchy(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        kind: lsp::extras::Hierarchy,
    ) -> Oneshot<Vec<lsp::extras::HierarchyItem>> {
        match self.lsp_target(doc) {
            Some((client, uri)) => {
                lsp::extras::prepare_hierarchy(&client, &uri, text, offset, kind)
            }
            None => Oneshot::ready(Vec::new()),
        }
    }

    /// The parents or the children of an item of a hierarchy.
    pub fn expand_hierarchy(
        &self,
        doc: &DocRef,
        item: &lsp::extras::HierarchyItem,
        kind: lsp::extras::Hierarchy,
        up: bool,
    ) -> Oneshot<Vec<lsp::extras::HierarchyItem>> {
        match self.lsp_target(doc) {
            Some((client, _)) => lsp::extras::expand_hierarchy(&client, item, kind, up),
            None => Oneshot::ready(Vec::new()),
        }
    }

    pub fn definition(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, |p, d, t| p.definition(d, t, offset))
    }

    pub fn references(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        self.locate(doc, text, offset, |p, d, t| p.references(d, t, offset))
    }

    /// Whether Reformat Code works for a language: with its provider, or by the IDE itself.
    pub fn formats(&self, language: &str) -> bool {
        self.handles(language) || language == "json"
    }

    /// The edits that reformat the document: its provider's, else the IDE's own for JSON.
    pub fn format(&self, doc: &DocRef, text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
        if html::is_markup(&doc.language) {
            return self.format_page(doc, text);
        }
        if let Some(answer) = self.with(&doc.language, |p, _| p.format(doc, text)) {
            return answer;
        }
        Oneshot::ready(match doc.language.as_str() {
            "json" => format::json(text).map(|new| format::edits_between(text, &new)),
            _ => Err(tr!("no_formatter_language").into()),
        })
    }

    pub fn organize_imports(
        &self,
        doc: &DocRef,
        text: &str,
    ) -> Oneshot<Result<Vec<TextEdit>, String>> {
        self.with(&doc.language, |p, _| p.organize_imports(doc, text))
            .unwrap_or_else(|| Oneshot::ready(Ok(Vec::new())))
    }

    pub fn resolve(&self, doc: &DocRef, item: &CompletionItem) -> Oneshot<Option<CompletionItem>> {
        // The item came from the provider of the code the completion was asked in.
        let target = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.completed_in.get(&doc.key).cloned())
            .unwrap_or_else(|| doc.clone());
        self.with(&target.language, |p, _| p.resolve(&target, item))
            .unwrap_or_else(|| Oneshot::ready(None))
    }

    /// Reformat Code for a page: the page by the IDE, then each block of code by its language,
    /// put back under its tag.
    fn format_page(&self, doc: &DocRef, text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CALLS: AtomicU64 = AtomicU64::new(1);
        let page = if doc.language == "pug" {
            html::format::reformat_pug(text)
        } else {
            html::format::reformat(text)
        };
        let regions = html::embedded::regions(&page, &doc.language);
        let call = CALLS.fetch_add(1, Ordering::Relaxed);
        let mut blocks = Vec::new();
        let mut parts = Vec::new();
        if let Ok(mut inner) = self.inner.lock() {
            let code_blocks = regions.iter().filter(|r| r.analyzed && r.prefix.is_empty());
            for (index, region) in code_blocks.enumerate() {
                let Some(block) = html::embedded::Block::of(&page, region.range.clone()) else {
                    continue;
                };
                if !inner.providers.iter().any(|p| p.handles(region.language)) {
                    continue;
                }
                // A document of its own for each block, never seen before.
                let key = embedded_key(doc.key, region.language, call * 1000 + index as u64);
                let tag = format!(".format-{call}-{index}");
                let scratch = embedded_doc(doc, region.language, key, &tag);
                Self::update_in(&mut inner, &scratch, &block.code, 1);
                inner.scratch.push((key, Instant::now()));
                let answer = inner
                    .providers
                    .iter_mut()
                    .find(|p| p.handles(region.language))
                    .map(|p| p.format(&scratch, &block.code));
                if let Some(answer) = answer {
                    blocks.push(block);
                    parts.push(answer);
                }
            }
        }
        let original = text.to_string();
        if parts.is_empty() {
            return Oneshot::ready(Ok(format::edits_between(&original, &page)));
        }
        join(parts, self.repaint.clone(), move |results| {
            let mut out = page;
            // From the last block, so the places of the others stay right. A block its language
            // could not format stays as the page put it.
            for (block, result) in blocks.iter().zip(results).rev() {
                let Some(Ok(edits)) = result else {
                    continue;
                };
                let code = format::apply(&block.code, &edits);
                out.replace_range(block.body.clone(), &block.indent(&code));
            }
            Ok(format::edits_between(&original, &out))
        })
    }

    pub fn status(&self, language: &str) -> String {
        self.with(language, |p, _| p.status()).unwrap_or_default()
    }
}

/// Fuzzy match of `query` in `label`, case-insensitive: all the characters of the query in
/// order, preferring the start of the label and of the words (camelCase, snake_case). None
/// when it does not match; a higher score is a better match.
#[cfg_attr(not(test), allow(dead_code))]
pub fn fuzzy_score(query: &str, label: &str) -> Option<i32> {
    fuzzy_match(query, label).map(|(score, _)| score)
}

/// Like [`fuzzy_score`], with the positions (in chars) of the label the query matched.
pub fn fuzzy_match(query: &str, label: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let label_chars: Vec<char> = label.chars().collect();
    let mut score = 0;
    let mut index = 0;
    let mut previous: Option<usize> = None;
    let mut positions = Vec::new();
    for q in query.chars() {
        let q_lower = q.to_lowercase().next().unwrap_or(q);
        let found = (index..label_chars.len())
            .find(|&i| label_chars[i].to_lowercase().next() == Some(q_lower))?;
        let boundary = found == 0
            || !label_chars[found - 1].is_alphanumeric()
            || (label_chars[found].is_uppercase() && label_chars[found - 1].is_lowercase());
        score += 10;
        if found == 0 {
            score += 20;
        }
        if boundary {
            score += 8;
        }
        if label_chars[found] == q {
            score += 2;
        }
        if previous.is_some_and(|p| p + 1 == found) {
            score += 6;
        }
        score -= (found - index) as i32;
        previous = Some(found);
        positions.push(found);
        index = found + 1;
    }
    if label.to_lowercase().starts_with(&query.to_lowercase()) {
        score += 40;
    }
    Some((score, positions))
}

#[cfg(test)]
#[path = "../../test/languages/mod/tests.rs"]
mod tests;
