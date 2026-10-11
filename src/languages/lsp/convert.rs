//! The answers of a language server, turned into what the editor shows: completion items,
//! hovers, places, edits, quick fixes and the structure of a file. Shared by the languages that
//! have a server (JavaScript and TypeScript, Rust).

use crate::languages::{
    Completion, CompletionItem, FileEdit, ItemKind, Location, QuickFix, Severity, Symbol, TextEdit,
    lsp::{self, from_position},
};
use serde_json::Value;
use std::sync::Arc;

pub(crate) fn severity(code: u8) -> Severity {
    match code {
        1 => Severity::Error,
        2 => Severity::Warning,
        3 => Severity::Info,
        _ => Severity::Hint,
    }
}

pub(crate) fn item_kind(code: u64) -> ItemKind {
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
pub(crate) fn markup_text(value: &Value) -> String {
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
pub(crate) fn plain(markdown: &str) -> String {
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
pub(crate) fn split_hover(markdown: &str) -> (String, String) {
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
pub(crate) fn locations_from_server(result: &Value, uri: &str, snapshot: &str) -> Vec<Location> {
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
pub(crate) fn workspace_edit(result: &Value, uri: &str, snapshot: &str) -> Vec<FileEdit> {
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
pub(crate) fn fixes_from_server(result: &Value, uri: &str, snapshot: &str) -> Vec<QuickFix> {
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

pub(crate) fn symbol_kind(code: u64) -> ItemKind {
    match code {
        5 | 23 => ItemKind::Class,
        6 | 9 => ItemKind::Method,
        7 | 8 => ItemKind::Property,
        11 => ItemKind::Interface,
        12 => ItemKind::Function,
        13 => ItemKind::Variable,
        14 | 22 | 10 => ItemKind::Constant,
        2..=4 => ItemKind::Module,
        _ => ItemKind::Text,
    }
}

/// The structure of a `textDocument/documentSymbol` answer, nested or flat, in document order.
pub(crate) fn symbols_from_server(result: &Value, snapshot: &str) -> Vec<Symbol> {
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

pub(crate) fn completion_from_server(
    result: &Value,
    text: &str,
    from: usize,
) -> Option<Completion> {
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
