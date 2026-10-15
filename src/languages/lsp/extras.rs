//! What a language server adds to the basics, asked for by the editor: the signature of the call
//! being typed (Parameter Info), the hints on the code (types, names of arguments), the
//! refactorings of a selection, and the hierarchies of types and calls.
//!
//! The functions take the client and the address of the document a provider gives with
//! [`crate::languages::Provider::lsp_target`], so every language with a server shares them.

use crate::languages::{
    FileEdit, ItemKind, Oneshot,
    lsp::{
        Client, from_position,
        convert::{markup_text, plain, symbol_kind, workspace_edit},
        to_position, uri_to_path,
    },
};
use serde_json::{Value, json};
use std::{ops::Range, path::PathBuf, sync::Arc};

// ---- Parameter Info ------------------------------------------------------------------------

/// The signature of the call at the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub label: String,
    /// The parameters, as char ranges of the label.
    pub parameters: Vec<Range<usize>>,
    /// The parameter being typed.
    pub active: Option<usize>,
    pub doc: String,
    /// Which signature of how many (an overloaded function).
    pub index: usize,
    pub count: usize,
}

/// A char range of `label` for a protocol parameter label: a substring, or UTF-16 offsets.
fn parameter_range(label: &str, parameter: &Value) -> Option<Range<usize>> {
    match &parameter["label"] {
        Value::String(part) => {
            let at = label.find(part.as_str())?;
            let start = label[..at].chars().count();
            Some(start..start + part.chars().count())
        }
        Value::Array(pair) => {
            let from = pair.first()?.as_u64()? as usize;
            let to = pair.get(1)?.as_u64()? as usize;
            // UTF-16 offsets into chars.
            let units: Vec<usize> = label
                .chars()
                .scan(0, |acc, c| {
                    let at = *acc;
                    *acc += c.len_utf16();
                    Some(at)
                })
                .collect();
            let char_of = |unit: usize| units.iter().position(|&u| u >= unit).unwrap_or(units.len());
            Some(char_of(from)..char_of(to))
        }
        _ => None,
    }
}

pub fn signature_from_server(result: &Value) -> Option<Signature> {
    let signatures = result["signatures"].as_array()?;
    let count = signatures.len();
    let index = result["activeSignature"].as_u64().unwrap_or(0) as usize;
    let signature = signatures.get(index).or_else(|| signatures.first())?;
    let label = signature["label"].as_str()?.to_string();
    let parameters: Vec<Range<usize>> = signature["parameters"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|p| parameter_range(&label, p))
                .collect()
        })
        .unwrap_or_default();
    let active = signature["activeParameter"]
        .as_u64()
        .or(result["activeParameter"].as_u64())
        .map(|a| a as usize)
        .filter(|a| *a < parameters.len());
    Some(Signature {
        label,
        parameters,
        active,
        doc: plain(&markup_text(&signature["documentation"])),
        index: index.min(count.saturating_sub(1)),
        count,
    })
}

pub fn signature_help(
    client: &Arc<Client>,
    uri: &str,
    text: &str,
    offset: usize,
) -> Oneshot<Option<Signature>> {
    let (line, character) = to_position(text, offset);
    let answer = Oneshot::pending();
    let slot = answer.clone();
    client.request(
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        move |result| slot.fulfill(result.ok().and_then(|r| signature_from_server(&r))),
    );
    answer
}

// ---- Inlay hints ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintKind {
    Type,
    Parameter,
    Other,
}

/// A hint shown in the code, before the char at `offset`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InlayHint {
    pub offset: usize,
    pub label: String,
    pub kind: HintKind,
}

pub fn hints_from_server(result: &Value, text: &str) -> Vec<InlayHint> {
    let Some(list) = result.as_array() else {
        return Vec::new();
    };
    let mut hints: Vec<InlayHint> = list
        .iter()
        .filter_map(|hint| {
            let line = hint["position"]["line"].as_u64()? as usize;
            let character = hint["position"]["character"].as_u64()? as usize;
            let label = match &hint["label"] {
                Value::String(label) => label.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p["value"].as_str())
                    .collect::<String>(),
                _ => return None,
            };
            let label = label.trim().to_string();
            if label.is_empty() {
                return None;
            }
            let kind = match hint["kind"].as_u64() {
                Some(1) => HintKind::Type,
                Some(2) => HintKind::Parameter,
                _ => HintKind::Other,
            };
            Some(InlayHint {
                offset: from_position(text, line, character),
                label,
                kind,
            })
        })
        .collect();
    hints.sort_by_key(|h| h.offset);
    hints
}

/// The hints of the lines `first..=last`.
pub fn inlay_hints(
    client: &Arc<Client>,
    uri: &str,
    text: &str,
    first: usize,
    last: usize,
) -> Oneshot<Vec<InlayHint>> {
    let answer = Oneshot::pending();
    let slot = answer.clone();
    let snapshot = text.to_string();
    client.request(
        "textDocument/inlayHint",
        json!({
            "textDocument": {"uri": uri},
            "range": {"start": {"line": first, "character": 0}, "end": {"line": last + 1, "character": 0}},
        }),
        move |result| slot.fulfill(result.map(|r| hints_from_server(&r, &snapshot)).unwrap_or_default()),
    );
    answer
}

// ---- Refactorings --------------------------------------------------------------------------

/// A refactoring of the server: what it changes in the files of the project, and the files it
/// creates (move to a new file).
#[derive(Clone, Debug)]
pub struct Refactoring {
    pub title: String,
    /// The changes, those of the document the request was made in with a path None.
    pub files: Vec<FileEdit>,
    pub creates: Vec<PathBuf>,
}

/// The refactorings in the answer of `textDocument/codeAction`: the actions that carry their
/// edits (the ones that only run a command are left out).
pub fn refactorings_from_server(result: &Value, uri: &str, snapshot: &str) -> Vec<Refactoring> {
    let Some(actions) = result.as_array() else {
        return Vec::new();
    };
    actions
        .iter()
        .filter_map(|action| {
            let title = action["title"].as_str()?.to_string();
            let edit = action.get("edit").filter(|e| e.is_object())?;
            let creates: Vec<PathBuf> = edit["documentChanges"]
                .as_array()
                .map(|changes| {
                    changes
                        .iter()
                        .filter(|c| c["kind"] == "create")
                        .filter_map(|c| uri_to_path(c["uri"].as_str()?))
                        .collect()
                })
                .unwrap_or_default();
            let files = workspace_edit(edit, uri, snapshot);
            (!files.is_empty() || !creates.is_empty()).then_some(Refactoring {
                title,
                files,
                creates,
            })
        })
        .collect()
}

/// The refactorings the server offers for a range (extract, inline, move…), with their edits.
pub fn refactorings(
    client: &Arc<Client>,
    uri: &str,
    text: &str,
    range: Range<usize>,
) -> Oneshot<Vec<Refactoring>> {
    let start = to_position(text, range.start);
    let end = to_position(text, range.end);
    let params = json!({
        "textDocument": {"uri": uri},
        "range": {
            "start": {"line": start.0, "character": start.1},
            "end": {"line": end.0, "character": end.1},
        },
        "context": {"diagnostics": [], "only": ["refactor"], "triggerKind": 1},
    });
    let answer = Oneshot::pending();
    let slot = answer.clone();
    let (snapshot, uri) = (text.to_string(), uri.to_string());
    client.request("textDocument/codeAction", params, move |result| {
        slot.fulfill(
            result
                .map(|r| refactorings_from_server(&r, &uri, &snapshot))
                .unwrap_or_default(),
        )
    });
    answer
}

// ---- Hierarchies ---------------------------------------------------------------------------

/// Which hierarchy: the types (supertypes and subtypes) or the calls (callers and callees).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hierarchy {
    Types,
    Calls,
}

/// A node of a hierarchy: a type or a function, with where it is.
#[derive(Clone, Debug)]
pub struct HierarchyItem {
    pub name: String,
    pub detail: String,
    pub kind: ItemKind,
    pub path: Option<PathBuf>,
    /// 0-based line and UTF-16 column of its name.
    pub line: usize,
    pub column: usize,
    /// The item as the server sent it, to ask for its parents or children.
    pub raw: Arc<Value>,
}

fn item_from_server(raw: &Value) -> Option<HierarchyItem> {
    let range = if raw["selectionRange"].is_object() {
        &raw["selectionRange"]
    } else {
        &raw["range"]
    };
    Some(HierarchyItem {
        name: raw["name"].as_str()?.to_string(),
        detail: raw["detail"].as_str().unwrap_or("").to_string(),
        kind: symbol_kind(raw["kind"].as_u64().unwrap_or(0)),
        path: raw["uri"].as_str().and_then(uri_to_path),
        line: range["start"]["line"].as_u64()? as usize,
        column: range["start"]["character"].as_u64().unwrap_or(0) as usize,
        raw: Arc::new(raw.clone()),
    })
}

/// The item of the type or the function at `offset`.
pub fn prepare_hierarchy(
    client: &Arc<Client>,
    uri: &str,
    text: &str,
    offset: usize,
    kind: Hierarchy,
) -> Oneshot<Vec<HierarchyItem>> {
    let (line, character) = to_position(text, offset);
    let method = match kind {
        Hierarchy::Types => "textDocument/prepareTypeHierarchy",
        Hierarchy::Calls => "textDocument/prepareCallHierarchy",
    };
    let answer = Oneshot::pending();
    let slot = answer.clone();
    client.request(
        method,
        json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        move |result| {
            slot.fulfill(
                result
                    .ok()
                    .and_then(|r| r.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(item_from_server)
                    .collect(),
            )
        },
    );
    answer
}

/// The parents (`up`: supertypes, callers) or the children (subtypes, callees) of an item.
pub fn expand_hierarchy(
    client: &Arc<Client>,
    item: &HierarchyItem,
    kind: Hierarchy,
    up: bool,
) -> Oneshot<Vec<HierarchyItem>> {
    let (method, field) = match (kind, up) {
        (Hierarchy::Types, true) => ("typeHierarchy/supertypes", None),
        (Hierarchy::Types, false) => ("typeHierarchy/subtypes", None),
        (Hierarchy::Calls, true) => ("callHierarchy/incomingCalls", Some("from")),
        (Hierarchy::Calls, false) => ("callHierarchy/outgoingCalls", Some("to")),
    };
    let answer = Oneshot::pending();
    let slot = answer.clone();
    client.request(method, json!({"item": *item.raw}), move |result| {
        let list = result
            .ok()
            .and_then(|r| r.as_array().cloned())
            .unwrap_or_default();
        slot.fulfill(
            list.iter()
                .filter_map(|entry| match field {
                    Some(field) => item_from_server(&entry[field]),
                    None => item_from_server(entry),
                })
                .collect(),
        )
    });
    answer
}


#[cfg(test)]
#[path = "../../../test/languages/lsp/extras/tests.rs"]
mod tests;
