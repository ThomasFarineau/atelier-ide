//! What the IDE knows of a Rust file by itself, with tree-sitter: syntax errors, the structure,
//! the occurrences of a name, keywords, snippets and the names of the file for completion, and
//! where a name of the file is declared. `rust-analyzer` knows better and takes over once it
//! runs; this is what answers before, or without it.

use crate::languages::{Completion, CompletionItem, Diagnostic, ItemKind, Location, Symbol};
use std::{collections::HashMap, ops::Range};
use tree_sitter::{Node, Parser, Tree};

/// Bigger files are left to the server: parsing them on every change would slow typing.
pub const MAX_TEXT: usize = 2_000_000;
const MAX_SYMBOLS: usize = 2000;

pub fn parse(text: &str) -> Option<Tree> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

/// The text of a node on one line: `(a: u32,\n b: u32)` becomes `(a: u32, b: u32)`.
fn flat(node: Node, src: &str) -> String {
    text(node, src)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- Syntax errors -----------------------------------------------------------------------

pub fn syntax_errors(src: &str) -> Vec<Diagnostic> {
    match parse(src) {
        Some(tree) => crate::languages::syntax::errors(
            &tree,
            src,
            &["identifier", "type_identifier", "field_identifier"],
        ),
        None => Vec::new(),
    }
}

// ---- Structure ---------------------------------------------------------------------------

pub fn symbols(src: &str) -> Vec<Symbol> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect(tree.root_node(), src, 0, &mut out);
    out
}

/// The `fn` signature after the name: `(a: u32) -> bool`.
fn signature(node: Node, src: &str) -> String {
    let params = node
        .child_by_field_name("parameters")
        .map_or_else(|| "()".to_string(), |p| flat(p, src));
    match node.child_by_field_name("return_type") {
        Some(ret) => format!("{params} -> {}", flat(ret, src)),
        None => params,
    }
}

fn push(
    out: &mut Vec<Symbol>,
    name: Node,
    src: &str,
    kind: ItemKind,
    detail: String,
    depth: usize,
) {
    out.push(Symbol {
        name: text(name, src).to_string(),
        kind,
        detail,
        range: name.byte_range(),
        depth,
    });
}

fn collect(parent: Node, src: &str, depth: usize, out: &mut Vec<Symbol>) {
    let mut cursor = parent.walk();
    for node in parent.named_children(&mut cursor) {
        if out.len() >= MAX_SYMBOLS {
            return;
        }
        let name = node.child_by_field_name("name");
        let body = node.child_by_field_name("body");
        match (node.kind(), name) {
            ("function_item" | "function_signature_item", Some(name)) => {
                let kind = if depth > 0 && parent.kind() == "declaration_list" {
                    ItemKind::Method
                } else {
                    ItemKind::Function
                };
                push(out, name, src, kind, signature(node, src), depth);
            }
            ("struct_item" | "union_item", Some(name)) => {
                push(out, name, src, ItemKind::Class, String::new(), depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("enum_item", Some(name)) => {
                push(out, name, src, ItemKind::Enum, String::new(), depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("enum_variant", Some(name)) => {
                let detail = body.map(|b| flat(b, src)).unwrap_or_default();
                push(out, name, src, ItemKind::Constant, detail, depth);
            }
            ("field_declaration", Some(name)) => {
                let detail = node
                    .child_by_field_name("type")
                    .map(|t| flat(t, src))
                    .unwrap_or_default();
                push(out, name, src, ItemKind::Property, detail, depth);
            }
            ("trait_item", Some(name)) => {
                push(out, name, src, ItemKind::Interface, "trait".into(), depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("mod_item", Some(name)) => {
                push(out, name, src, ItemKind::Module, "mod".into(), depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("const_item" | "static_item", Some(name)) => {
                let detail = node
                    .child_by_field_name("type")
                    .map(|t| flat(t, src))
                    .unwrap_or_default();
                push(out, name, src, ItemKind::Constant, detail, depth);
            }
            ("type_item" | "associated_type", Some(name)) => {
                push(out, name, src, ItemKind::Interface, "type".into(), depth);
            }
            ("macro_definition", Some(name)) => {
                push(
                    out,
                    name,
                    src,
                    ItemKind::Function,
                    "macro_rules!".into(),
                    depth,
                );
            }
            ("impl_item", _) => {
                let Some(ty) = node.child_by_field_name("type") else {
                    continue;
                };
                // `impl Trait for Type` is named after both, as IntelliJ shows it.
                let label = match node.child_by_field_name("trait") {
                    Some(t) => format!("{} for {}", flat(t, src), flat(ty, src)),
                    None => flat(ty, src),
                };
                out.push(Symbol {
                    name: label,
                    kind: ItemKind::Class,
                    detail: "impl".into(),
                    range: ty.byte_range(),
                    depth,
                });
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("declaration_list" | "field_declaration_list" | "enum_variant_list", _) => {
                collect(node, src, depth, out)
            }
            _ => {}
        }
    }
}

// ---- Names -------------------------------------------------------------------------------

fn is_name(node: Node) -> bool {
    matches!(
        node.kind(),
        "identifier" | "type_identifier" | "field_identifier" | "shorthand_field_identifier"
    )
}

/// The name under the caret, or just before it.
fn name_at(tree: &Tree, offset: usize) -> Option<Node<'_>> {
    let root = tree.root_node();
    let found = |at: usize| {
        root.named_descendant_for_byte_range(at, at)
            .filter(|n| is_name(*n))
    };
    found(offset).or_else(|| offset.checked_sub(1).and_then(found))
}

fn all_names<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if is_name(node) {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        all_names(child, out);
    }
}

/// Every place of the file with the name under the caret. Without types, a name is the same
/// name wherever it is written: a fallback until the server answers.
pub fn highlights(src: &str, offset: usize) -> Vec<Range<usize>> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let Some(node) = name_at(&tree, offset) else {
        return Vec::new();
    };
    let wanted = text(node, src);
    let field = matches!(
        node.kind(),
        "field_identifier" | "shorthand_field_identifier"
    );
    let mut names = Vec::new();
    all_names(tree.root_node(), &mut names);
    names
        .into_iter()
        .filter(|n| text(*n, src) == wanted)
        .filter(|n| matches!(n.kind(), "field_identifier" | "shorthand_field_identifier") == field)
        .map(|n| n.byte_range())
        .collect()
}

/// A name declared in the file: by an item, a `let`, a parameter or a closure parameter.
struct Declaration {
    name: Range<usize>,
    kind: ItemKind,
    detail: String,
    /// Where the name can be used: the whole file for an item, the enclosing block for a local.
    scope: Range<usize>,
}

fn declarations(tree: &Tree, src: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    declare(tree.root_node(), src, &mut out);
    out
}

/// The names a pattern binds: `(a, Some(b))` binds `a` and `b`.
fn bound<'a>(pattern: Node<'a>, out: &mut Vec<Node<'a>>) {
    match pattern.kind() {
        "identifier" => out.push(pattern),
        // `Some(x)`: the type is not a binding.
        "tuple_struct_pattern" => {
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor).skip(1) {
                bound(child, out);
            }
        }
        "field_pattern" => match pattern.child_by_field_name("pattern") {
            Some(inner) => bound(inner, out),
            None => {
                if let Some(name) = pattern.child_by_field_name("name") {
                    out.push(name);
                }
            }
        },
        "scoped_identifier" | "scoped_type_identifier" => {}
        _ => {
            let mut cursor = pattern.walk();
            for child in pattern.named_children(&mut cursor) {
                bound(child, out);
            }
        }
    }
}

fn enclosing_block(node: Node) -> Range<usize> {
    let mut current = node.parent();
    while let Some(n) = current {
        if matches!(
            n.kind(),
            "block" | "function_item" | "closure_expression" | "match_arm" | "for_expression"
        ) {
            return n.byte_range();
        }
        current = n.parent();
    }
    0..usize::MAX
}

fn declare(node: Node, src: &str, out: &mut Vec<Declaration>) {
    let item = |kind| {
        node.child_by_field_name("name").map(|name| Declaration {
            name: name.byte_range(),
            kind,
            detail: String::new(),
            scope: 0..usize::MAX,
        })
    };
    let found = match node.kind() {
        "function_item" | "function_signature_item" => item(ItemKind::Function).map(|mut d| {
            d.detail = signature(node, src);
            d
        }),
        "struct_item" | "union_item" => item(ItemKind::Class),
        "enum_item" => item(ItemKind::Enum),
        "enum_variant" => item(ItemKind::Constant),
        "trait_item" | "type_item" | "associated_type" => item(ItemKind::Interface),
        "mod_item" => item(ItemKind::Module),
        "const_item" | "static_item" => item(ItemKind::Constant),
        "macro_definition" => item(ItemKind::Function),
        "field_declaration" => item(ItemKind::Property),
        _ => None,
    };
    out.extend(found);
    let locals = match node.kind() {
        "let_declaration" | "parameter" | "closure_parameters" | "for_expression"
        | "if_let_expression" | "let_condition" | "match_arm" => {
            let pattern = match node.kind() {
                "closure_parameters" => Some(node),
                "match_arm" => node.child_by_field_name("pattern"),
                _ => node.child_by_field_name("pattern"),
            };
            let mut names = Vec::new();
            if let Some(pattern) = pattern {
                bound(pattern, &mut names);
            }
            let detail = node
                .child_by_field_name("type")
                .map(|t| flat(t, src))
                .unwrap_or_default();
            let kind = if node.kind() == "parameter" {
                ItemKind::Parameter
            } else {
                ItemKind::Variable
            };
            names
                .into_iter()
                .map(|n| Declaration {
                    name: n.byte_range(),
                    kind,
                    detail: detail.clone(),
                    scope: enclosing_block(n),
                })
                .collect()
        }
        _ => Vec::new(),
    };
    out.extend(locals);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        declare(child, src, out);
    }
}

/// Where the name under the caret is declared in the file: the closest local before it, else
/// an item of that name.
pub fn definition(src: &str, offset: usize) -> Vec<Location> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let Some(node) = name_at(&tree, offset) else {
        return Vec::new();
    };
    let wanted = text(node, src);
    let at = node.start_byte();
    let all = declarations(&tree, src);
    let same: Vec<&Declaration> = all
        .iter()
        .filter(|d| &src[d.name.clone()] == wanted)
        .collect();
    // The name under the caret is a declaration itself: nothing to go to.
    if same.iter().any(|d| d.name == node.byte_range()) {
        return Vec::new();
    }
    let local = same
        .iter()
        .filter(|d| d.scope != (0..usize::MAX) && d.scope.contains(&at) && d.name.start < at)
        .max_by_key(|d| d.name.start);
    let chosen: Vec<&&Declaration> = match local {
        Some(d) => vec![d],
        None => same.iter().filter(|d| d.scope == (0..usize::MAX)).collect(),
    };
    chosen
        .into_iter()
        .map(|d| Location::in_text(None, src, d.name.clone()))
        .collect()
}

// ---- Completion --------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

/// Common names of the standard library and its prelude.
const PRELUDE: &[(&str, ItemKind)] = &[
    ("Option", ItemKind::Enum),
    ("Some", ItemKind::Constant),
    ("None", ItemKind::Constant),
    ("Result", ItemKind::Enum),
    ("Ok", ItemKind::Constant),
    ("Err", ItemKind::Constant),
    ("Vec", ItemKind::Class),
    ("String", ItemKind::Class),
    ("Box", ItemKind::Class),
    ("HashMap", ItemKind::Class),
    ("HashSet", ItemKind::Class),
    ("BTreeMap", ItemKind::Class),
    ("Default", ItemKind::Interface),
    ("Clone", ItemKind::Interface),
    ("Debug", ItemKind::Interface),
    ("Display", ItemKind::Interface),
    ("Iterator", ItemKind::Interface),
    ("println!", ItemKind::Function),
    ("format!", ItemKind::Function),
    ("vec!", ItemKind::Function),
    ("assert!", ItemKind::Function),
    ("assert_eq!", ItemKind::Function),
    ("panic!", ItemKind::Function),
    ("todo!", ItemKind::Function),
];

/// Code templates: (trigger, text with `$0` where the caret goes, key of the description in the
/// locales).
const SNIPPETS: &[(&str, &str, &str)] = &[
    ("fn", "fn $0() {\n    \n}", "rust.snippet.fn"),
    ("pfn", "pub fn $0() {\n    \n}", "rust.snippet.pfn"),
    ("main", "fn main() {\n    $0\n}", "rust.snippet.main"),
    ("test", "#[test]\nfn $0() {\n    \n}", "rust.snippet.test"),
    (
        "tmod",
        "#[cfg(test)]\nmod tests {\n    use super::*;\n\n    $0\n}",
        "rust.snippet.tmod",
    ),
    ("struct", "struct $0 {\n    \n}", "rust.snippet.struct"),
    ("enum", "enum $0 {\n    \n}", "rust.snippet.enum"),
    ("impl", "impl $0 {\n    \n}", "rust.snippet.impl"),
    ("trait", "trait $0 {\n    \n}", "rust.snippet.trait"),
    ("match", "match $0 {\n    \n}", "rust.snippet.match"),
    (
        "iflet",
        "if let Some($0) =  {\n    \n}",
        "rust.snippet.iflet",
    ),
    ("for", "for $0 in  {\n    \n}", "rust.snippet.for"),
    ("println", "println!(\"{}\", $0);", "rust.snippet.println"),
    ("derive", "#[derive($0)]", "rust.snippet.derive"),
];

/// The text of a template and where the caret goes in it.
fn expand(template: &str) -> (String, usize) {
    let caret = template.find("$0").unwrap_or(template.len());
    (template.replacen("$0", "", 1), caret)
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn word_start(text: &str, offset: usize) -> usize {
    let mut from = offset;
    for (i, c) in text[..offset].char_indices().rev() {
        if !is_ident(c) {
            break;
        }
        from = i;
    }
    from
}

/// Whether the caret is in a comment or a string, where completion is not offered.
fn in_comment_or_string(tree: &Tree, offset: usize) -> bool {
    let at = offset.saturating_sub(1);
    let mut node = tree.root_node().descendant_for_byte_range(at, at);
    while let Some(n) = node {
        if matches!(
            n.kind(),
            "line_comment"
                | "block_comment"
                | "string_literal"
                | "raw_string_literal"
                | "char_literal"
        ) {
            return n.start_byte() < offset;
        }
        node = n.parent();
    }
    false
}

/// Keywords, snippets, the prelude and the names of the file. After `.` or `::` only the server
/// knows the members: nothing then.
pub fn complete(src: &str, offset: usize, explicit: bool) -> Option<Completion> {
    let offset = offset.min(src.len());
    let from = word_start(src, offset);
    let typed = &src[from..offset];
    let before = src[..from].trim_end_matches(' ');
    if before.ends_with('.') || before.ends_with("::") || before.ends_with('\'') {
        return None;
    }
    if typed.is_empty() && !explicit {
        return None;
    }
    let tree = parse(src)?;
    if in_comment_or_string(&tree, offset) {
        return None;
    }
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    // The names of the file, the closest before the caret first.
    let mut declared = declarations(&tree, src);
    declared.sort_by_key(|d| offset.abs_diff(d.name.start));
    for d in declared {
        let name = &src[d.name.clone()];
        let visible =
            d.scope.contains(&offset) && (d.scope == (0..usize::MAX) || d.name.end <= from);
        if !visible || name == typed || seen.insert(name.to_string(), ()).is_some() {
            continue;
        }
        let mut item = CompletionItem::new(name, d.kind).rank(20);
        if !d.detail.is_empty() {
            item = item.detail(d.detail);
        }
        if d.kind == ItemKind::Function && !src[offset..].starts_with('(') {
            item = item.insert(format!("{name}()"), Some(name.len() + 1));
        }
        items.push(item);
    }
    for (name, kind) in PRELUDE {
        if seen.insert((*name).to_string(), ()).is_none() {
            let mut item = CompletionItem::new(*name, *kind).rank(40);
            if name.ends_with('!') {
                item = item.insert(format!("{name}()"), Some(name.len() + 1));
            }
            items.push(item);
        }
    }
    for keyword in KEYWORDS {
        items.push(CompletionItem::new(*keyword, ItemKind::Keyword).rank(60));
    }
    for (trigger, template, key) in SNIPPETS {
        let (body, caret) = expand(template);
        items.push(
            CompletionItem::new(*trigger, ItemKind::Snippet)
                .detail(crate::i18n::t(key))
                .insert(body, Some(caret))
                .rank(70),
        );
    }
    Some(Completion { from, items })
}

#[cfg(test)]
#[path = "../../../test/languages/rust/analysis/tests.rs"]
mod tests;
