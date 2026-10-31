//! What the IDE knows of a Python file by itself, with tree-sitter: syntax errors, the
//! structure, the occurrences of a name in its scope, where a name of the file is declared
//! (`self.x` included), and completion of keywords, builtins, names, members of `self` and
//! IntelliJ's templates. pyright knows better and takes over once it runs.

use crate::languages::{Completion, CompletionItem, Diagnostic, ItemKind, Location, Symbol};
use std::ops::Range;
use tree_sitter::{Node, Parser, Tree};

pub const MAX_TEXT: usize = 2_000_000;
const MAX_SYMBOLS: usize = 2000;

pub fn parse(text: &str) -> Option<Tree> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
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

// ---- Syntax errors -----------------------------------------------------------------------

pub fn syntax_errors(src: &str) -> Vec<Diagnostic> {
    match parse(src) {
        Some(tree) => crate::languages::syntax::errors(&tree, src, &["identifier"]),
        None => Vec::new(),
    }
}

// ---- Structure ---------------------------------------------------------------------------

pub fn symbols(src: &str) -> Vec<Symbol> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect(tree.root_node(), src, 0, false, &mut out);
    out
}

/// A `NAME` all in capitals is a constant.
fn constant(name: &str) -> bool {
    name.chars().any(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
}

fn collect(parent: Node, src: &str, depth: usize, in_class: bool, out: &mut Vec<Symbol>) {
    let mut cursor = parent.walk();
    for child in parent.named_children(&mut cursor) {
        if out.len() >= MAX_SYMBOLS {
            return;
        }
        let node = if child.kind() == "decorated_definition" {
            match child.child_by_field_name("definition") {
                Some(definition) => definition,
                None => continue,
            }
        } else {
            child
        };
        match node.kind() {
            "function_definition" => {
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                let mut detail = node
                    .child_by_field_name("parameters")
                    .map(|p| flat(p, src))
                    .unwrap_or_default();
                if let Some(returns) = node.child_by_field_name("return_type") {
                    detail.push_str(&format!(" -> {}", flat(returns, src)));
                }
                out.push(Symbol {
                    name: text(name, src).to_string(),
                    kind: if in_class {
                        ItemKind::Method
                    } else {
                        ItemKind::Function
                    },
                    detail,
                    range: name.byte_range(),
                    depth,
                });
            }
            "class_definition" => {
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                out.push(Symbol {
                    name: text(name, src).to_string(),
                    kind: ItemKind::Class,
                    detail: node
                        .child_by_field_name("superclasses")
                        .map(|s| flat(s, src))
                        .unwrap_or_default(),
                    range: name.byte_range(),
                    depth,
                });
                if let Some(body) = node.child_by_field_name("body") {
                    collect(body, src, depth + 1, true, out);
                }
            }
            // The names a module or a class assigns.
            "expression_statement" => {
                let Some(assignment) = node.named_child(0).filter(|n| n.kind() == "assignment")
                else {
                    continue;
                };
                let Some(left) = assignment.child_by_field_name("left") else {
                    continue;
                };
                if left.kind() != "identifier" {
                    continue;
                }
                let name = text(left, src);
                out.push(Symbol {
                    name: name.to_string(),
                    kind: if constant(name) {
                        ItemKind::Constant
                    } else if in_class {
                        ItemKind::Property
                    } else {
                        ItemKind::Variable
                    },
                    detail: assignment
                        .child_by_field_name("type")
                        .map(|t| flat(t, src))
                        .unwrap_or_default(),
                    range: left.byte_range(),
                    depth,
                });
            }
            "if_statement" if depth == 0 => {
                // `if __name__ == "__main__":` and the definitions of a module under a condition.
                if let Some(body) = node.child_by_field_name("consequence") {
                    collect(body, src, depth, in_class, out);
                }
            }
            _ => {}
        }
    }
}

// ---- Names and scopes ----------------------------------------------------------------------

/// The identifier at `offset` (or just before it).
fn name_at(tree: &Tree, offset: usize) -> Option<Node<'_>> {
    let root = tree.root_node();
    [offset, offset.saturating_sub(1)]
        .into_iter()
        .find_map(|at| {
            root.descendant_for_byte_range(at, at)
                .filter(|n| n.kind() == "identifier")
        })
}

/// The name after the dot of `a.name`: a member, not a variable.
fn is_member(node: Node) -> bool {
    node.parent().is_some_and(|p| {
        p.kind() == "attribute"
            && p.child_by_field_name("attribute")
                .is_some_and(|a| a.id() == node.id())
    })
}

/// The name of a keyword argument (`f(name=1)`): no variable either.
fn is_keyword_name(node: Node) -> bool {
    node.parent().is_some_and(|p| {
        p.kind() == "keyword_argument"
            && p.child_by_field_name("name")
                .is_some_and(|n| n.id() == node.id())
    })
}

fn all_identifiers<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if node.kind() == "identifier" {
        out.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        all_identifiers(child, out);
    }
}

/// A name declared, and where it can be seen.
#[derive(Debug)]
struct Declaration {
    name: String,
    at: Range<usize>,
    /// The function (or lambda, or comprehension) it belongs to; the whole file for a module.
    scope: Range<usize>,
}

const FILE: Range<usize> = 0..usize::MAX;

/// The scope a name declared at `node` belongs to: the innermost function or lambda. A class
/// body is no scope for the functions in it, as in Python.
fn scope_of(node: Node) -> Range<usize> {
    let mut current = node.parent();
    while let Some(n) = current {
        if matches!(n.kind(), "function_definition" | "lambda") {
            return n.byte_range();
        }
        current = n.parent();
    }
    FILE
}

/// The names a pattern binds: `a`, `a, b`, `(a, *b)`.
fn bound<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    match node.kind() {
        "identifier" => out.push(node),
        "attribute" | "subscript" => {}
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                bound(child, out);
            }
        }
    }
}

fn declarations(tree: &Tree, src: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    declare(tree.root_node(), src, &mut out);
    out
}

fn declare(node: Node, src: &str, out: &mut Vec<Declaration>) {
    let mut add = |name: Node, scope: Range<usize>| {
        out.push(Declaration {
            name: text(name, src).to_string(),
            at: name.byte_range(),
            scope,
        });
    };
    match node.kind() {
        "function_definition" | "class_definition" => {
            if let Some(name) = node.child_by_field_name("name") {
                add(name, scope_of(node));
            }
        }
        "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
            if let Some(left) = node.child_by_field_name("left") {
                let mut names = Vec::new();
                bound(left, &mut names);
                // A comprehension has a scope of its own.
                let scope = if node.kind() == "for_in_clause" {
                    node.parent().map_or(FILE, |p| p.byte_range())
                } else {
                    scope_of(node)
                };
                for name in names {
                    add(name, scope.clone());
                }
            }
        }
        "as_pattern" => {
            if let Some(alias) = node.child_by_field_name("alias") {
                let mut names = Vec::new();
                bound(alias, &mut names);
                for name in names {
                    add(name, scope_of(node));
                }
            }
        }
        "parameters" | "lambda_parameters" => {
            let scope = node.parent().map_or(FILE, |p| p.byte_range());
            let mut cursor = node.walk();
            for parameter in node.named_children(&mut cursor) {
                let name = match parameter.kind() {
                    "identifier" => Some(parameter),
                    "default_parameter" | "typed_default_parameter" => {
                        parameter.child_by_field_name("name")
                    }
                    _ => {
                        let mut names = Vec::new();
                        bound(parameter, &mut names);
                        names.into_iter().next()
                    }
                };
                if let Some(name) = name {
                    add(name, scope.clone());
                }
            }
        }
        "import_statement" | "import_from_statement" => {
            let scope = scope_of(node);
            let mut cursor = node.walk();
            for name in node.children_by_field_name("name", &mut cursor) {
                let bound = match name.kind() {
                    "aliased_import" => name.child_by_field_name("alias"),
                    // `import a.b` binds `a`.
                    "dotted_name" => name.named_child(0),
                    _ => None,
                };
                if let Some(bound) = bound {
                    add(bound, scope.clone());
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        declare(child, src, out);
    }
}

/// The declarations a use of `name` at `offset` can mean: the ones of the innermost scope that
/// holds both, first first.
fn visible<'a>(all: &'a [Declaration], name: &str, offset: usize) -> Vec<&'a Declaration> {
    let candidates: Vec<&Declaration> = all
        .iter()
        .filter(|d| d.name == name && d.scope.contains(&offset))
        .collect();
    let innermost = candidates
        .iter()
        .map(|d| d.scope.end.saturating_sub(d.scope.start))
        .min();
    candidates
        .into_iter()
        .filter(|d| Some(d.scope.end.saturating_sub(d.scope.start)) == innermost)
        .collect()
}

/// The class a node is in.
fn enclosing_class(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(n) = current {
        if n.kind() == "class_definition" {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// What `self.name` can be: the methods of the class and the `self.name = …` in it.
fn members<'a>(class: Node<'a>, src: &str) -> Vec<(String, Node<'a>, ItemKind)> {
    let mut out = Vec::new();
    let Some(body) = class.child_by_field_name("body") else {
        return out;
    };
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        let node = if child.kind() == "decorated_definition" {
            child.child_by_field_name("definition").unwrap_or(child)
        } else {
            child
        };
        if node.kind() == "function_definition"
            && let Some(name) = node.child_by_field_name("name")
        {
            out.push((text(name, src).to_string(), name, ItemKind::Method));
        }
    }
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        if node.kind() == "assignment"
            && let Some(left) = node.child_by_field_name("left")
            && left.kind() == "attribute"
            && left
                .child_by_field_name("object")
                .is_some_and(|o| text(o, src) == "self")
            && let Some(attribute) = left.child_by_field_name("attribute")
        {
            let name = text(attribute, src).to_string();
            if !out.iter().any(|(n, ..)| *n == name) {
                out.push((name, attribute, ItemKind::Property));
            }
        }
        let mut cursor = node.walk();
        // Not into the classes inside.
        for child in node.named_children(&mut cursor) {
            if child.kind() != "class_definition" {
                stack.push(child);
            }
        }
    }
    out
}

/// `self` before the dot of a member.
fn on_self<'a>(node: Node<'a>, src: &str) -> Option<Node<'a>> {
    let attribute = node.parent()?;
    let object = attribute.child_by_field_name("object")?;
    (text(object, src) == "self").then_some(attribute)
}

pub fn definition(src: &str, offset: usize) -> Vec<Location> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let Some(node) = name_at(&tree, offset) else {
        return Vec::new();
    };
    let name = text(node, src);
    let location = |range: Range<usize>| Location::in_text(None, src, range);
    if is_member(node) {
        // `self.name`: in the class.
        return on_self(node, src)
            .and_then(enclosing_class)
            .map(|class| {
                members(class, src)
                    .into_iter()
                    .filter(|(n, ..)| n == name)
                    .take(1)
                    .map(|(_, at, _)| location(at.byte_range()))
                    .collect()
            })
            .unwrap_or_default();
    }
    if is_keyword_name(node) {
        return Vec::new();
    }
    let all = declarations(&tree, src);
    visible(&all, name, node.start_byte())
        .into_iter()
        .take(1)
        .map(|d| location(d.at.clone()))
        .collect()
}

pub fn highlights(src: &str, offset: usize) -> Vec<Range<usize>> {
    let Some(tree) = parse(src) else {
        return Vec::new();
    };
    let Some(node) = name_at(&tree, offset) else {
        return Vec::new();
    };
    let name = text(node, src);
    let mut names = Vec::new();
    all_identifiers(tree.root_node(), &mut names);
    if is_member(node) {
        // The same member of `self` in the class.
        let Some(class) = on_self(node, src).and_then(enclosing_class) else {
            return Vec::new();
        };
        let range = class.byte_range();
        return names
            .into_iter()
            .filter(|n| text(*n, src) == name && range.contains(&n.start_byte()))
            .filter(|n| {
                is_member(*n) && on_self(*n, src).is_some()
                    || n.parent()
                        .is_some_and(|p| p.kind() == "function_definition")
            })
            .map(|n| n.byte_range())
            .collect();
    }
    let all = declarations(&tree, src);
    let scope = visible(&all, name, node.start_byte())
        .first()
        .map_or(FILE, |d| d.scope.clone());
    // Within the scope, the names that mean the same: not the ones a deeper scope declares.
    names
        .into_iter()
        .filter(|n| text(*n, src) == name && !is_member(*n) && !is_keyword_name(*n))
        .filter(|n| scope.contains(&n.start_byte()))
        .filter(|n| {
            visible(&all, name, n.start_byte())
                .first()
                .map_or(scope == FILE, |d| d.scope == scope)
        })
        .map(|n| n.byte_range())
        .collect()
}

// ---- Completion ----------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "match", "case", "nonlocal", "not", "or", "pass", "raise", "return",
    "try", "while", "with", "yield", "self",
];

const BUILTINS: &[&str] = &[
    "abs",
    "all",
    "any",
    "bool",
    "bytes",
    "callable",
    "chr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "map",
    "max",
    "min",
    "next",
    "object",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "classmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
    "Exception",
    "ValueError",
    "TypeError",
    "KeyError",
    "IndexError",
    "RuntimeError",
    "NotImplementedError",
    "AttributeError",
    "StopIteration",
    "OSError",
    "FileNotFoundError",
];

/// Templates: (abbreviation, description, text with `$` where the caret goes).
const SNIPPETS: &[(&str, &str, &str)] = &[
    ("def", "Function", "def $():\n    pass"),
    (
        "class",
        "Class",
        "class $:\n    def __init__(self):\n        pass",
    ),
    ("init", "__init__", "def __init__(self$):\n    pass"),
    (
        "main",
        "if __name__ == \"__main__\"",
        "if __name__ == \"__main__\":\n    $",
    ),
    ("for", "for … in …", "for $ in :\n    pass"),
    ("forr", "for … in range()", "for i in range($):\n    pass"),
    ("with", "with … as …", "with $ as f:\n    pass"),
    (
        "try",
        "try / except",
        "try:\n    $\nexcept Exception as e:\n    raise",
    ),
    ("prop", "@property", "@property\ndef $(self):\n    return"),
];

/// The text of a template with the indentation of its line, and where the caret goes.
fn expand(template: &str, indent: &str) -> (String, usize) {
    let text = template.replace('\n', &format!("\n{indent}"));
    let caret = text.find('$').unwrap_or(text.len());
    (text.replacen('$', "", 1), caret)
}

pub fn word_start(text: &str, offset: usize) -> usize {
    let mut from = offset;
    for (i, c) in text[..offset].char_indices().rev() {
        if !(c.is_alphanumeric() || c == '_') {
            break;
        }
        from = i;
    }
    from
}

fn in_comment_or_string(tree: &Tree, offset: usize) -> bool {
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset.saturating_sub(1), offset.saturating_sub(1));
    while let Some(n) = node {
        if matches!(n.kind(), "comment" | "string_content" | "string_start") {
            return true;
        }
        if n.kind() == "string" {
            // Inside the quotes, out of an interpolation of an f-string.
            return n.start_byte() < offset && offset < n.end_byte();
        }
        if n.kind() == "interpolation" {
            return false;
        }
        node = n.parent();
    }
    false
}

pub fn complete(src: &str, offset: usize, explicit: bool) -> Option<Completion> {
    let tree = parse(src)?;
    let offset = offset.min(src.len());
    let from = word_start(src, offset);
    let typed = &src[from..offset];
    let after_dot = src[..from].ends_with('.');
    if typed.is_empty() && !explicit && !after_dot {
        return None;
    }
    if in_comment_or_string(&tree, offset) {
        return None;
    }
    let mut items = Vec::new();
    if after_dot {
        // `self.`: the members of the class; anything else is the server's.
        let object = src[..from - 1]
            .trim_end()
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if object != "self" {
            return None;
        }
        let node = tree
            .root_node()
            .descendant_for_byte_range(from.saturating_sub(1), from.saturating_sub(1))?;
        let class = enclosing_class(node)?;
        for (name, _, kind) in members(class, src) {
            items.push(CompletionItem::new(name, kind).rank(10));
        }
        return Some(Completion { from, items });
    }
    let all = declarations(&tree, src);
    let mut seen = std::collections::HashSet::new();
    // The caret at the very end of a function is still in it.
    let in_scope = |d: &&Declaration| d.scope.start <= offset && offset <= d.scope.end;
    for declaration in all.iter().filter(in_scope) {
        if declaration.at.start <= offset && offset <= declaration.at.end {
            continue;
        }
        if seen.insert(declaration.name.clone()) {
            let kind = if constant(&declaration.name) {
                ItemKind::Constant
            } else {
                ItemKind::Variable
            };
            items.push(CompletionItem::new(declaration.name.clone(), kind).rank(10));
        }
    }
    for keyword in KEYWORDS {
        items.push(CompletionItem::new(*keyword, ItemKind::Keyword).rank(40));
    }
    for builtin in BUILTINS {
        let kind = if builtin.starts_with(char::is_uppercase) {
            ItemKind::Class
        } else {
            ItemKind::Function
        };
        items.push(CompletionItem::new(*builtin, kind).rank(30));
    }
    let line_start = src[..from].rfind('\n').map_or(0, |i| i + 1);
    let before = &src[line_start..from];
    // Templates only where a statement starts.
    if before.trim().is_empty() {
        for (abbreviation, description, template) in SNIPPETS {
            let (insert, caret) = expand(template, before);
            items.push(
                CompletionItem::new(*abbreviation, ItemKind::Snippet)
                    .detail(*description)
                    .insert(insert, Some(caret))
                    .rank(20),
            );
        }
    }
    Some(Completion { from, items })
}
