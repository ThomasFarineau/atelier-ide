//! What one file says about the structure of the project, read from its syntax tree (no AI): the
//! things it defines, the modules it imports, and the names it calls, extends or implements.
//! Resolving those names to places of the project is done later, on all files together
//! ([`super::build`]).

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

/// A thing a file defines.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Sym {
    /// The name, qualified by its type for a method (`Counter::increment`).
    pub name: String,
    /// `function`, `method`, `class`, `struct`, `enum`, `trait`, `interface`, `type`, `module`, `macro`.
    pub kind: String,
    /// 1-based line.
    pub line: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Calls,
    Extends,
    Implements,
    Instantiates,
}

/// A name a file uses: from the symbol it is in (None for the file itself).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Ref {
    pub from: Option<usize>,
    pub name: String,
    /// `Type` of `Type::new()`, which tells apart methods of the same name.
    pub qualifier: Option<String>,
    /// What a method is called on (`self` of `self.open()`): the name alone says little, as
    /// every type has a `contains` or a `new`.
    #[serde(default)]
    pub receiver: Option<String>,
    pub kind: RefKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub language: String,
    pub symbols: Vec<Sym>,
    /// Module specifiers: `./a`, `crate::b::C`, `mod d;` (as `mod:d`), or a Markdown link target.
    pub imports: Vec<String>,
    pub refs: Vec<Ref>,
}

/// The languages the graph reads, by file extension.
pub fn language_of(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "js" | "jsx" | "mjs" | "cjs" => Some("js"),
        "ts" | "mts" | "cts" => Some("ts"),
        "tsx" => Some("tsx"),
        "rs" => Some("rs"),
        "md" | "markdown" => Some("md"),
        _ => None,
    }
}

/// Files longer than this are skipped: generated bundles say nothing about the structure.
pub const MAX_SOURCE: u64 = 1_000_000;

pub fn extract(language: &str, text: &str) -> Facts {
    let mut facts = Facts {
        language: language.to_string(),
        ..Facts::default()
    };
    match language {
        "md" => markdown(text, &mut facts),
        "rs" => parse(
            tree_sitter_rust::LANGUAGE.into(),
            text,
            &mut facts,
            rust_walk,
        ),
        "ts" => parse(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            text,
            &mut facts,
            js_walk,
        ),
        "tsx" => parse(
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            text,
            &mut facts,
            js_walk,
        ),
        _ => parse(
            tree_sitter_javascript::LANGUAGE.into(),
            text,
            &mut facts,
            js_walk,
        ),
    }
    facts
}

fn parse(
    language: tree_sitter::Language,
    text: &str,
    facts: &mut Facts,
    walk: fn(Node, &str, &mut Facts, Option<usize>),
) {
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return;
    }
    if let Some(tree) = parser.parse(text, None) {
        walk(tree.root_node(), text, facts, None);
    }
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn field_text<'a>(node: Node, name: &str, src: &'a str) -> Option<&'a str> {
    node.child_by_field_name(name).map(|n| text(n, src))
}

fn add(facts: &mut Facts, name: &str, kind: &str, node: Node) -> usize {
    facts.symbols.push(Sym {
        name: name.to_string(),
        kind: kind.to_string(),
        line: node.start_position().row + 1,
    });
    facts.symbols.len() - 1
}

fn reference(
    facts: &mut Facts,
    from: Option<usize>,
    name: &str,
    qualifier: Option<&str>,
    kind: RefKind,
) {
    if !name.is_empty() {
        facts.refs.push(Ref {
            from,
            name: name.to_string(),
            qualifier: qualifier.map(str::to_string),
            receiver: None,
            kind,
        });
    }
}

/// A call on a value: `object.name()`.
fn method_call(facts: &mut Facts, from: Option<usize>, name: &str, receiver: &str) {
    if !name.is_empty() {
        facts.refs.push(Ref {
            from,
            name: name.to_string(),
            qualifier: None,
            receiver: Some(receiver.to_string()),
            kind: RefKind::Calls,
        });
    }
}

fn children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

// ---- JavaScript and TypeScript --------------------------------------------------------------

fn unquote(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`')
}

/// The last name of `a.b.c`, with what precedes it.
fn last_name(s: &str) -> (Option<&str>, &str) {
    match s.rsplit_once('.') {
        Some((before, name)) => (before.rsplit('.').next(), name),
        None => (None, s),
    }
}

fn js_walk(node: Node, src: &str, facts: &mut Facts, owner: Option<usize>) {
    let mut next_owner = owner;
    match node.kind() {
        "import_statement" | "export_statement" => {
            if let Some(source) = node.child_by_field_name("source") {
                facts.imports.push(unquote(text(source, src)).to_string());
            }
        }
        "function_declaration" | "generator_function_declaration" => {
            if let Some(name) = field_text(node, "name", src) {
                next_owner = Some(add(facts, name, "function", node));
            }
        }
        "class_declaration" | "abstract_class_declaration" | "class" => {
            if let Some(name) = field_text(node, "name", src) {
                let class = add(facts, name, "class", node);
                for heritage in children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "class_heritage")
                {
                    for part in children(heritage) {
                        match part.kind() {
                            "extends_clause" => {
                                for target in children(part).into_iter().filter(|c| {
                                    matches!(
                                        c.kind(),
                                        "identifier"
                                            | "member_expression"
                                            | "type_identifier"
                                            | "nested_type_identifier"
                                    )
                                }) {
                                    let (q, n) = last_name(text(target, src));
                                    reference(facts, Some(class), n, q, RefKind::Extends);
                                }
                            }
                            "implements_clause" => {
                                for target in children(part).into_iter().filter(|c| {
                                    matches!(
                                        c.kind(),
                                        "type_identifier"
                                            | "nested_type_identifier"
                                            | "generic_type"
                                    )
                                }) {
                                    let (q, n) = last_name(
                                        text(target, src).split('<').next().unwrap_or(""),
                                    );
                                    reference(facts, Some(class), n, q, RefKind::Implements);
                                }
                            }
                            "identifier" | "member_expression" => {
                                let (q, n) = last_name(text(part, src));
                                reference(facts, Some(class), n, q, RefKind::Extends);
                            }
                            _ => {}
                        }
                    }
                }
                next_owner = Some(class);
            }
        }
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            if let Some(name) = field_text(node, "name", src) {
                let qualified = match owner.map(|o| facts.symbols[o].name.clone()) {
                    Some(class) if facts.symbols[owner.unwrap_or(0)].kind == "class" => {
                        format!("{class}::{name}")
                    }
                    _ => name.to_string(),
                };
                next_owner = Some(add(facts, &qualified, "method", node));
            }
        }
        "interface_declaration" => {
            if let Some(name) = field_text(node, "name", src) {
                let symbol = add(facts, name, "interface", node);
                for clause in children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "extends_type_clause")
                {
                    for target in children(clause).into_iter().filter(|c| {
                        matches!(
                            c.kind(),
                            "type_identifier" | "nested_type_identifier" | "generic_type"
                        )
                    }) {
                        let (q, n) = last_name(text(target, src).split('<').next().unwrap_or(""));
                        reference(facts, Some(symbol), n, q, RefKind::Extends);
                    }
                }
                next_owner = Some(symbol);
            }
        }
        "type_alias_declaration" => {
            if let Some(name) = field_text(node, "name", src) {
                add(facts, name, "type", node);
            }
        }
        "enum_declaration" => {
            if let Some(name) = field_text(node, "name", src) {
                add(facts, name, "enum", node);
            }
        }
        "variable_declarator" => {
            let function_value = node.child_by_field_name("value").is_some_and(|v| {
                matches!(
                    v.kind(),
                    "arrow_function" | "function_expression" | "function"
                )
            });
            if function_value && let Some(name) = field_text(node, "name", src) {
                next_owner = Some(add(facts, name, "function", node));
            }
        }
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                let callee = text(function, src);
                if callee == "require" {
                    if let Some(arguments) = node.child_by_field_name("arguments")
                        && let Some(first) = children(arguments)
                            .into_iter()
                            .find(|c| c.kind() == "string")
                    {
                        facts.imports.push(unquote(text(first, src)).to_string());
                    }
                } else if function.kind() == "identifier" {
                    reference(facts, owner, callee, None, RefKind::Calls);
                } else if function.kind() == "member_expression" {
                    let name = field_text(function, "property", src).unwrap_or("");
                    let object = field_text(function, "object", src).unwrap_or("");
                    // `Counter.create()` is a call on a class; `x.add()` on a value, which is unknown.
                    if object.chars().next().is_some_and(char::is_uppercase)
                        && !object.contains('.')
                    {
                        reference(facts, owner, name, Some(object), RefKind::Calls);
                    } else {
                        method_call(facts, owner, name, object);
                    }
                }
            }
        }
        "new_expression" => {
            if let Some(constructor) = field_text(node, "constructor", src) {
                let (q, n) = last_name(constructor);
                reference(facts, owner, n, q, RefKind::Instantiates);
            }
        }
        _ => {}
    }
    for child in children(node) {
        js_walk(child, src, facts, next_owner);
    }
}

// ---- Rust ---------------------------------------------------------------------------------

/// The paths a `use` brings in: `a::{b, c::d}` is `a::b` and `a::c::d`.
fn use_paths(node: Node, src: &str, prefix: &str, out: &mut Vec<String>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}::{name}")
        }
    };
    match node.kind() {
        "scoped_identifier" | "identifier" | "crate" | "self" | "super" => {
            out.push(join(text(node, src)))
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                use_paths(path, src, prefix, out);
            }
        }
        "use_wildcard" => {
            let path = children(node)
                .into_iter()
                .find(|c| c.kind() != "*" && c.kind() != "::");
            out.push(path.map_or_else(|| prefix.to_string(), |p| join(text(p, src))));
        }
        "scoped_use_list" => {
            let base = node
                .child_by_field_name("path")
                .map_or(prefix.to_string(), |p| join(text(p, src)));
            if let Some(list) = node.child_by_field_name("list") {
                use_paths(list, src, &base, out);
            }
        }
        "use_list" => {
            for child in children(node).into_iter().filter(|c| c.is_named()) {
                use_paths(child, src, prefix, out);
            }
        }
        _ => {}
    }
}

fn rust_walk(node: Node, src: &str, facts: &mut Facts, owner: Option<usize>) {
    let mut next_owner = owner;
    match node.kind() {
        "use_declaration" => {
            if let Some(argument) = node.child_by_field_name("argument") {
                let mut paths = Vec::new();
                use_paths(argument, src, "", &mut paths);
                facts.imports.extend(paths);
            }
        }
        "mod_item" => {
            if let Some(name) = field_text(node, "name", src) {
                if node.child_by_field_name("body").is_none() {
                    facts.imports.push(format!("mod:{name}"));
                }
                next_owner = Some(add(facts, name, "module", node));
            }
        }
        "function_item" | "function_signature_item" => {
            if let Some(name) = field_text(node, "name", src) {
                next_owner = Some(match owner.map(|o| &facts.symbols[o]) {
                    Some(parent) if matches!(parent.kind.as_str(), "struct" | "trait" | "enum") => {
                        let qualified = format!("{}::{name}", parent.name);
                        add(facts, &qualified, "method", node)
                    }
                    _ => add(facts, name, "function", node),
                });
            }
        }
        "struct_item" | "enum_item" | "trait_item" | "type_item" | "union_item" => {
            if let Some(name) = field_text(node, "name", src) {
                let kind = match node.kind() {
                    "struct_item" | "union_item" => "struct",
                    "enum_item" => "enum",
                    "trait_item" => "trait",
                    _ => "type",
                };
                next_owner = Some(add(facts, name, kind, node));
            }
        }
        "macro_definition" => {
            if let Some(name) = field_text(node, "name", src) {
                add(facts, name, "macro", node);
            }
        }
        "impl_item" => {
            // The methods belong to the type; `impl Trait for Type` makes the type implement it.
            if let Some(ty) = field_text(node, "type", src) {
                let ty = ty.split('<').next().unwrap_or(ty).trim();
                let name = ty.rsplit("::").next().unwrap_or(ty).trim_start_matches('&');
                let existing = facts.symbols.iter().position(|s| {
                    s.name == name && matches!(s.kind.as_str(), "struct" | "enum" | "trait")
                });
                let owner_symbol = existing.unwrap_or_else(|| add(facts, name, "struct", node));
                if let Some(tr) = field_text(node, "trait", src) {
                    let tr = tr.split('<').next().unwrap_or(tr);
                    let (q, n) = (None, tr.rsplit("::").next().unwrap_or(tr));
                    reference(facts, Some(owner_symbol), n, q, RefKind::Implements);
                }
                next_owner = Some(owner_symbol);
            }
        }
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                match function.kind() {
                    "identifier" => {
                        reference(facts, owner, text(function, src), None, RefKind::Calls)
                    }
                    "scoped_identifier" => {
                        let name = field_text(function, "name", src).unwrap_or("");
                        let qualifier = function
                            .child_by_field_name("path")
                            .map(|p| text(p, src).rsplit("::").next().unwrap_or("").to_string());
                        reference(facts, owner, name, qualifier.as_deref(), RefKind::Calls);
                    }
                    "field_expression" => {
                        if let Some(name) = field_text(function, "field", src) {
                            method_call(
                                facts,
                                owner,
                                name,
                                field_text(function, "value", src).unwrap_or(""),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    for child in children(node) {
        rust_walk(child, src, facts, next_owner);
    }
}

// ---- Markdown -----------------------------------------------------------------------------

/// Headings are the symbols of a document and relative links its imports. Read line by line: a
/// syntax tree of every document would cost more than it tells.
fn markdown(text: &str, facts: &mut Facts) {
    let mut fenced = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=3).contains(&level) && trimmed[level..].starts_with(' ') {
            let title = trimmed[level..].trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                facts.symbols.push(Sym {
                    name: title.to_string(),
                    kind: "heading".into(),
                    line: index + 1,
                });
            }
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            let after = &rest[at + 2..];
            let Some(end) = after.find(')') else { break };
            let target = after[..end].split(['#', ' ']).next().unwrap_or("");
            if !target.is_empty() && !target.contains("://") && !target.starts_with("mailto:") {
                facts.imports.push(target.to_string());
            }
            rest = &after[end..];
        }
    }
}

#[cfg(test)]
#[path = "../../test/graph/extract/tests.rs"]
mod tests;
