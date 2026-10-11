//! What the IDE knows of a Java file by itself, with tree-sitter: syntax errors, the structure,
//! the occurrences of a name, where a name of the file is declared, and the keywords, snippets
//! and names of the file for completion. jdtls knows better and takes over once it runs.

use crate::languages::{Completion, CompletionItem, Diagnostic, ItemKind, Location, Symbol};
use std::{collections::HashSet, ops::Range};
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
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

/// The text of a node on one line.
fn flat(node: Node, src: &str) -> String {
    text(node, src)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn syntax_errors(src: &str) -> Vec<Diagnostic> {
    match parse(src) {
        Some(tree) => {
            crate::languages::syntax::errors(&tree, src, &["identifier", "type_identifier"])
        }
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

/// `(int a, String b) : boolean`, as IntelliJ shows a method in the structure.
fn signature(node: Node, src: &str) -> String {
    let params = node
        .child_by_field_name("parameters")
        .map_or_else(|| "()".to_string(), |p| flat(p, src));
    match node.child_by_field_name("type") {
        Some(ret) => format!("{params}: {}", flat(ret, src)),
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
            ("class_declaration" | "record_declaration", Some(name)) => {
                let detail = node
                    .child_by_field_name("parameters")
                    .map(|p| flat(p, src))
                    .unwrap_or_default();
                push(out, name, src, ItemKind::Class, detail, depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("interface_declaration" | "annotation_type_declaration", Some(name)) => {
                push(out, name, src, ItemKind::Interface, String::new(), depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("enum_declaration", Some(name)) => {
                push(out, name, src, ItemKind::Enum, String::new(), depth);
                if let Some(body) = body {
                    collect(body, src, depth + 1, out);
                }
            }
            ("enum_constant", Some(name)) => {
                push(out, name, src, ItemKind::Constant, String::new(), depth);
            }
            ("method_declaration" | "annotation_type_element_declaration", Some(name)) => {
                push(
                    out,
                    name,
                    src,
                    ItemKind::Method,
                    signature(node, src),
                    depth,
                );
            }
            ("constructor_declaration" | "compact_constructor_declaration", Some(name)) => {
                push(
                    out,
                    name,
                    src,
                    ItemKind::Method,
                    signature(node, src),
                    depth,
                );
            }
            ("field_declaration" | "constant_declaration", _) => {
                let ty = node
                    .child_by_field_name("type")
                    .map(|t| flat(t, src))
                    .unwrap_or_default();
                let mut inner = node.walk();
                for declarator in node.children_by_field_name("declarator", &mut inner) {
                    if let Some(name) = declarator.child_by_field_name("name") {
                        push(out, name, src, ItemKind::Property, ty.clone(), depth);
                    }
                }
            }
            (
                "class_body"
                | "interface_body"
                | "enum_body"
                | "enum_body_declarations"
                | "annotation_type_body",
                _,
            ) => collect(node, src, depth, out),
            _ => {}
        }
    }
}

// ---- Names -------------------------------------------------------------------------------

fn is_name(node: Node) -> bool {
    matches!(node.kind(), "identifier" | "type_identifier")
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

/// Whether a name is a member after a dot (`a.b`): the `b` of another object.
fn is_member(node: Node) -> bool {
    node.parent().is_some_and(|p| {
        matches!(p.kind(), "field_access" | "method_invocation")
            && p.child_by_field_name("object").is_some_and(|o| o != node)
            && p.child_by_field_name(if p.kind() == "field_access" {
                "field"
            } else {
                "name"
            }) == Some(node)
    })
}

/// Every place of the file with the name under the caret: a fallback until the server answers.
pub fn highlights(src: &str, offset: usize) -> Vec<Range<usize>> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let Some(node) = name_at(&tree, offset) else {
        return Vec::new();
    };
    let wanted = text(node, src);
    let member = is_member(node);
    let mut names = Vec::new();
    all_names(tree.root_node(), &mut names);
    names
        .into_iter()
        .filter(|n| text(*n, src) == wanted && (is_member(*n) == member || !member))
        .map(|n| n.byte_range())
        .collect()
}

/// A name declared in the file.
struct Declaration {
    name: Range<usize>,
    kind: ItemKind,
    detail: String,
    /// Where it can be used: the whole file for a type or a member, the block for a local.
    scope: Range<usize>,
}

const FILE: Range<usize> = 0..usize::MAX;

fn enclosing_block(node: Node) -> Range<usize> {
    let mut current = node.parent();
    while let Some(n) = current {
        if matches!(
            n.kind(),
            "block"
                | "method_declaration"
                | "constructor_declaration"
                | "lambda_expression"
                | "for_statement"
                | "enhanced_for_statement"
                | "catch_clause"
                | "try_with_resources_statement"
        ) {
            return n.byte_range();
        }
        current = n.parent();
    }
    FILE
}

fn declarations(tree: &Tree, src: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    declare(tree.root_node(), src, &mut out);
    out
}

fn declare(node: Node, src: &str, out: &mut Vec<Declaration>) {
    let named = |kind, detail: String, scope| {
        node.child_by_field_name("name").map(|name| Declaration {
            name: name.byte_range(),
            kind,
            detail,
            scope,
        })
    };
    let found = match node.kind() {
        "class_declaration" | "record_declaration" => named(ItemKind::Class, String::new(), FILE),
        "interface_declaration" | "annotation_type_declaration" => {
            named(ItemKind::Interface, String::new(), FILE)
        }
        "enum_declaration" => named(ItemKind::Enum, String::new(), FILE),
        "enum_constant" => named(ItemKind::Constant, String::new(), FILE),
        "method_declaration" => named(ItemKind::Method, signature(node, src), FILE),
        "formal_parameter" | "spread_parameter" | "catch_formal_parameter" => {
            let ty = node
                .child_by_field_name("type")
                .map(|t| flat(t, src))
                .unwrap_or_default();
            named(ItemKind::Parameter, ty, enclosing_block(node))
        }
        "enhanced_for_statement" => {
            let ty = node
                .child_by_field_name("type")
                .map(|t| flat(t, src))
                .unwrap_or_default();
            named(ItemKind::Variable, ty, node.byte_range())
        }
        _ => None,
    };
    out.extend(found);
    if matches!(
        node.kind(),
        "field_declaration" | "constant_declaration" | "local_variable_declaration"
    ) {
        let ty = node
            .child_by_field_name("type")
            .map(|t| flat(t, src))
            .unwrap_or_default();
        let (kind, scope) = if node.kind() == "local_variable_declaration" {
            (ItemKind::Variable, enclosing_block(node))
        } else {
            (ItemKind::Property, FILE)
        };
        let mut cursor = node.walk();
        for declarator in node.children_by_field_name("declarator", &mut cursor) {
            if let Some(name) = declarator.child_by_field_name("name") {
                out.push(Declaration {
                    name: name.byte_range(),
                    kind,
                    detail: ty.clone(),
                    scope: scope.clone(),
                });
            }
        }
    }
    // `x -> …` and `(a, b) -> …`: the parameters of a lambda.
    if node.kind() == "lambda_expression"
        && let Some(params) = node.child_by_field_name("parameters")
    {
        let mut names = Vec::new();
        if params.kind() == "identifier" {
            names.push(params);
        } else {
            let mut cursor = params.walk();
            names.extend(
                params
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() == "identifier"),
            );
        }
        for name in names {
            out.push(Declaration {
                name: name.byte_range(),
                kind: ItemKind::Parameter,
                detail: String::new(),
                scope: node.byte_range(),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        declare(child, src, out);
    }
}

/// Where the name under the caret is declared in the file: the closest local before it, else
/// a type or a member of that name.
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
    if same.iter().any(|d| d.name == node.byte_range()) {
        return Vec::new();
    }
    let local = (!is_member(node))
        .then(|| {
            same.iter()
                .filter(|d| d.scope != FILE && d.scope.contains(&at) && d.name.start < at)
                .max_by_key(|d| d.name.start)
        })
        .flatten();
    let chosen: Vec<&&Declaration> = match local {
        Some(d) => vec![d],
        None => same.iter().filter(|d| d.scope == FILE).collect(),
    };
    chosen
        .into_iter()
        .map(|d| Location::in_text(None, src, d.name.clone()))
        .collect()
}

// ---- Completion --------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "yield",
    "true",
    "false",
];

/// Common types of `java.lang` and `java.util`.
const COMMON: &[(&str, ItemKind)] = &[
    ("String", ItemKind::Class),
    ("Object", ItemKind::Class),
    ("Integer", ItemKind::Class),
    ("Long", ItemKind::Class),
    ("Double", ItemKind::Class),
    ("Boolean", ItemKind::Class),
    ("Character", ItemKind::Class),
    ("Math", ItemKind::Class),
    ("System", ItemKind::Class),
    ("StringBuilder", ItemKind::Class),
    ("Exception", ItemKind::Class),
    ("RuntimeException", ItemKind::Class),
    ("IllegalArgumentException", ItemKind::Class),
    ("IllegalStateException", ItemKind::Class),
    ("List", ItemKind::Interface),
    ("ArrayList", ItemKind::Class),
    ("Map", ItemKind::Interface),
    ("HashMap", ItemKind::Class),
    ("Set", ItemKind::Interface),
    ("HashSet", ItemKind::Class),
    ("Optional", ItemKind::Class),
    ("Override", ItemKind::Interface),
];

/// Code templates, as IntelliJ's live templates: (abbreviation, text with `$0` where the caret
/// goes, key of the description in the locales).
const SNIPPETS: &[(&str, &str, &str)] = &[
    (
        "psvm",
        "public static void main(String[] args) {\n    $0\n}",
        "java.snippet.psvm",
    ),
    (
        "main",
        "public static void main(String[] args) {\n    $0\n}",
        "java.snippet.psvm",
    ),
    ("sout", "System.out.println($0);", "java.snippet.sout"),
    ("serr", "System.err.println($0);", "java.snippet.serr"),
    (
        "fori",
        "for (int i = 0; i < $0; i++) {\n    \n}",
        "java.snippet.fori",
    ),
    (
        "iter",
        "for (var item : $0) {\n    \n}",
        "java.snippet.iter",
    ),
    ("ifn", "if ($0 == null) {\n    \n}", "java.snippet.ifn"),
    ("inn", "if ($0 != null) {\n    \n}", "java.snippet.inn"),
    (
        "try",
        "try {\n    $0\n} catch (Exception e) {\n    throw new RuntimeException(e);\n}",
        "java.snippet.try",
    ),
    ("class", "public class $0 {\n    \n}", "java.snippet.class"),
    ("record", "public record $0() {\n}", "java.snippet.record"),
    (
        "interface",
        "public interface $0 {\n    \n}",
        "java.snippet.interface",
    ),
    ("test", "@Test\nvoid $0() {\n    \n}", "java.snippet.test"),
];

fn expand(template: &str) -> (String, usize) {
    let caret = template.find("$0").unwrap_or(template.len());
    (template.replacen("$0", "", 1), caret)
}

pub fn word_start(text: &str, offset: usize) -> usize {
    let mut from = offset;
    for (i, c) in text[..offset].char_indices().rev() {
        if !(c.is_alphanumeric() || c == '_' || c == '$') {
            break;
        }
        from = i;
    }
    from
}

fn in_comment_or_string(tree: &Tree, offset: usize) -> bool {
    let at = offset.saturating_sub(1);
    let mut node = tree.root_node().descendant_for_byte_range(at, at);
    while let Some(n) = node {
        if matches!(
            n.kind(),
            "line_comment"
                | "block_comment"
                | "string_literal"
                | "text_block"
                | "character_literal"
        ) {
            return n.start_byte() < offset;
        }
        node = n.parent();
    }
    false
}

/// Keywords, live templates, common types and the names of the file. After a dot only the
/// server knows the members: nothing then.
pub fn complete(src: &str, offset: usize, explicit: bool) -> Option<Completion> {
    let offset = offset.min(src.len());
    let from = word_start(src, offset);
    let typed = &src[from..offset];
    if src[..from].trim_end_matches(' ').ends_with(['.', '@']) {
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
    let mut seen: HashSet<String> = HashSet::new();
    let mut declared = declarations(&tree, src);
    declared.sort_by_key(|d| offset.abs_diff(d.name.start));
    for d in declared {
        let name = &src[d.name.clone()];
        let visible = d.scope.contains(&offset) && (d.scope == FILE || d.name.end <= from);
        if !visible || name == typed || !seen.insert(name.to_string()) {
            continue;
        }
        let mut item = CompletionItem::new(name, d.kind).rank(20);
        if !d.detail.is_empty() {
            item = item.detail(d.detail);
        }
        if d.kind == ItemKind::Method && !src[offset..].starts_with('(') {
            item = item.insert(format!("{name}()"), Some(name.len() + 1));
        }
        items.push(item);
    }
    for (name, kind) in COMMON {
        if seen.insert((*name).to_string()) {
            items.push(CompletionItem::new(*name, *kind).rank(40));
        }
    }
    for keyword in KEYWORDS {
        items.push(CompletionItem::new(*keyword, ItemKind::Keyword).rank(60));
    }
    for (abbreviation, template, key) in SNIPPETS {
        let (body, caret) = expand(template);
        items.push(
            CompletionItem::new(*abbreviation, ItemKind::Snippet)
                .detail(crate::i18n::t(key))
                .insert(body, Some(caret))
                .rank(70),
        );
    }
    Some(Completion { from, items })
}

#[cfg(test)]
#[path = "../../../test/languages/java/analysis/tests.rs"]
mod tests;
