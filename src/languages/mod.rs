//! Language intelligence: diagnostics, completion and hover for the code in the editor.
//!
//! Each language brings a [`Provider`]. The [`Service`] is the hub the editor talks to: it
//! routes a document to the provider of its language and hands back results, either at once
//! (analysis done in Rust) or later (a language server answering in the background), through
//! a [`Oneshot`] the editor polls.

pub mod format;
pub mod java;
pub mod javascript;
pub mod lsp;
pub mod rust;
pub mod syntax;
pub mod typescript;

use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
}

/// The hub of language intelligence, shared by every editor.
#[derive(Default)]
pub struct Service {
    inner: Mutex<Inner>,
}

impl Service {
    /// The service with the providers of the supported languages, for a project folder.
    pub fn new(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let service = Self::default();
        service.add(Box::new(javascript::JavaScript::new(root, repaint.clone())));
        service.add(Box::new(rust::provider(root, repaint.clone())));
        service.add(Box::new(java::provider(root, repaint)));
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

    /// Tells the provider the document is at `version`. Cheap when nothing changed.
    pub fn update(&self, doc: &DocRef, text: &str, version: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(index) = inner
            .providers
            .iter()
            .position(|p| p.handles(&doc.language))
        else {
            return;
        };
        if inner.versions.get(&doc.key) == Some(&version)
            && inner.owners.get(&doc.key) == Some(&index)
        {
            return;
        }
        // A document whose language changed (saved under another name) leaves its old provider.
        if let Some(old) = inner.owners.insert(doc.key, index)
            && old != index
        {
            inner.providers[old].close(doc.key);
        }
        inner.versions.insert(doc.key, version);
        inner.providers[index].update(doc, text, version);
    }

    pub fn close(&self, key: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if let Some(index) = inner.owners.remove(&key) {
            inner.providers[index].close(key);
        }
        inner.versions.remove(&key);
    }

    pub fn diagnostics(&self, doc: &DocRef) -> Arc<Vec<Diagnostic>> {
        self.with(&doc.language, |p, _| p.diagnostics(doc.key))
            .unwrap_or_default()
    }

    pub fn complete(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        explicit: bool,
    ) -> Oneshot<Option<Completion>> {
        self.with(&doc.language, |p, _| {
            p.complete(doc, text, offset, explicit)
        })
        .unwrap_or_else(|| Oneshot::ready(None))
    }

    pub fn hover(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Option<Hover>> {
        self.with(&doc.language, |p, _| p.hover(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(None))
    }

    pub fn type_definition(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        self.with(&doc.language, |p, _| p.type_definition(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    pub fn implementation(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Location>> {
        self.with(&doc.language, |p, _| p.implementation(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    pub fn highlights(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
    ) -> Oneshot<Vec<Range<usize>>> {
        self.with(&doc.language, |p, _| p.highlights(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    pub fn symbols(&self, doc: &DocRef, text: &str) -> Oneshot<Vec<Symbol>> {
        self.with(&doc.language, |p, _| p.symbols(doc, text))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    pub fn rename(
        &self,
        doc: &DocRef,
        text: &str,
        offset: usize,
        new_name: &str,
    ) -> Oneshot<Result<Vec<FileEdit>, String>> {
        self.with(&doc.language, |p, _| p.rename(doc, text, offset, new_name))
            .unwrap_or_else(|| Oneshot::ready(Err(tr!("rename_not_supported_language").into())))
    }

    pub fn quick_fixes(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<QuickFix>> {
        self.with(&doc.language, |p, _| p.quick_fixes(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    pub fn definition(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        self.with(&doc.language, |p, _| p.definition(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    pub fn references(&self, doc: &DocRef, text: &str, offset: usize) -> Oneshot<Vec<Location>> {
        self.with(&doc.language, |p, _| p.references(doc, text, offset))
            .unwrap_or_else(|| Oneshot::ready(Vec::new()))
    }

    /// Whether Reformat Code works for a language: with its provider, or by the IDE itself.
    pub fn formats(&self, language: &str) -> bool {
        self.handles(language) || language == "json"
    }

    /// The edits that reformat the document: its provider's, else the IDE's own for JSON.
    pub fn format(&self, doc: &DocRef, text: &str) -> Oneshot<Result<Vec<TextEdit>, String>> {
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
        self.with(&doc.language, |p, _| p.resolve(doc, item))
            .unwrap_or_else(|| Oneshot::ready(None))
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
