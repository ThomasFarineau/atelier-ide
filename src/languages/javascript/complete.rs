//! Completion for JavaScript, from the syntax tree of the document and the built-in tables:
//! names in scope, members of the value before a dot, module paths in imports.

use super::{
    analysis::{Analysis, Decl, DeclKind, Prop, Ty, dialect},
    builtins,
};
use crate::languages::typescript;
use crate::languages::{Completion, CompletionItem, ItemKind};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tree_sitter::Node;

/// Largest document analyzed.
pub const MAX_TEXT: usize = 1_000_000;
const MAX_ITEMS: usize = 900;
/// Stands for the member being typed when there is nothing after the dot yet.
const PLACEHOLDER: &str = "__fx__";

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn clamp(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Start of the identifier ending at `offset`.
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

/// Candidates at `offset`. None when completion makes no sense there (a comment, a name being
/// declared, nothing typed yet).
pub fn complete(
    text: &str,
    offset: usize,
    explicit: bool,
    path: Option<&Path>,
) -> Option<Completion> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let offset = clamp(text, offset);
    let from = word_start(text, offset);
    let before = &text[..from];

    if let Some(result) = in_string_or_comment(text, offset, path) {
        return result;
    }

    // After a dot: the members of the value before it.
    if is_member_dot(before) {
        return member_completion(text, from, offset);
    }

    let typed = &text[from..offset];
    if typed.is_empty() && !explicit {
        return None;
    }
    // A name being declared: nothing to suggest.
    let last_word: String = before
        .trim_end()
        .chars()
        .rev()
        .take_while(|c| is_ident(*c))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let declaring: &[&str] = if dialect().is_ts() {
        &[
            "function",
            "class",
            "const",
            "let",
            "var",
            "interface",
            "type",
            "enum",
            "namespace",
        ]
    } else {
        &["function", "class", "const", "let", "var"]
    };
    if !explicit && declaring.contains(&last_word.as_str()) && before.ends_with(char::is_whitespace)
    {
        return None;
    }
    if typed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }

    if dialect().is_ts() {
        let end = word_end(text, offset);
        if let Some(completion) = type_completion(text, from, end, offset) {
            return Some(completion);
        }
        if let Some(completion) = object_key_completion(text, from, end, offset) {
            return Some(completion);
        }
    }

    let analysis = Analysis::new(text)?;
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (index, decl) in analysis.visible(offset).into_iter().enumerate() {
        // The word being typed is not a declaration of itself.
        if decl.name_range.end == offset && decl.name_range.start == from {
            continue;
        }
        if seen.insert(decl.name.clone()) {
            items.push(local_item(&analysis, decl, index));
        }
    }
    for item in builtins::global_items() {
        if seen.insert(item.label.clone()) {
            items.push(item);
        }
    }
    if dialect().is_ts() {
        for item in typescript::builtins::items() {
            if seen.insert(item.label.clone()) {
                items.push(item);
            }
        }
    }
    // Words of the file the analysis knows nothing about.
    for word in words(text, from, offset) {
        if seen.insert(word.clone()) {
            items.push(CompletionItem::new(word, ItemKind::Text).rank(90));
        }
    }
    items.truncate(MAX_ITEMS);
    Some(Completion { from, items })
}

fn word_end(text: &str, offset: usize) -> usize {
    let mut end = offset;
    while let Some(c) = text[end..].chars().next() {
        if !is_ident(c) {
            break;
        }
        end += c.len_utf8();
    }
    end
}

/// The analysis of the document with the word at `from..end` replaced by a name the parser
/// accepts, and the node of that name. The code around the caret is usually unfinished.
fn patched(text: &str, from: usize, end: usize, suffix: &str) -> Option<(Analysis, usize)> {
    let source = format!("{}{PLACEHOLDER}{suffix}{}", &text[..from], &text[end..]);
    let analysis = Analysis::new(&source)?;
    Some((analysis, from))
}

fn leaf_at<'a>(analysis: &'a Analysis, from: usize) -> Option<Node<'a>> {
    analysis
        .root()
        .descendant_for_byte_range(from, from + PLACEHOLDER.len())
}

/// Whether the node sits where a type goes.
fn in_type(leaf: Node, from: usize) -> bool {
    let mut node = Some(leaf);
    while let Some(n) = node {
        match n.kind() {
            "type_annotation"
            | "type_arguments"
            | "extends_type_clause"
            | "implements_clause"
            | "union_type"
            | "intersection_type"
            | "array_type"
            | "generic_type"
            | "constraint"
            | "default_type" => return true,
            "type_alias_declaration" => {
                return n
                    .child_by_field_name("value")
                    .is_some_and(|v| v.start_byte() <= from && from <= v.end_byte());
            }
            "as_expression" | "satisfies_expression" => {
                return n
                    .named_child(n.named_child_count().saturating_sub(1) as u32)
                    .is_some_and(|t| t.start_byte() <= from && from <= t.end_byte());
            }
            "statement_block" | "program" | "arguments" | "object" | "formal_parameters" => {
                return false;
            }
            _ => node = n.parent(),
        }
    }
    false
}

/// In a type: the types in scope, the type parameters around, and the ones of the language.
fn type_completion(text: &str, from: usize, end: usize, offset: usize) -> Option<Completion> {
    for suffix in ["", ";"] {
        let Some((analysis, from)) = patched(text, from, end, suffix) else {
            continue;
        };
        let Some(leaf) = leaf_at(&analysis, from) else {
            continue;
        };
        if leaf.kind() != "type_identifier" && !in_type(leaf, from) {
            continue;
        }
        let mut items: Vec<CompletionItem> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        // `<T>` of the generic function, class or interface around.
        let mut node = Some(leaf);
        while let Some(n) = node {
            if let Some(parameters) = n.child_by_field_name("type_parameters") {
                let mut cursor = parameters.walk();
                for parameter in parameters.named_children(&mut cursor) {
                    if let Some(name) = parameter.child_by_field_name("name") {
                        let name = analysis.text[name.byte_range()].to_string();
                        if seen.insert(name.clone()) {
                            items.push(
                                CompletionItem::new(name, ItemKind::Interface)
                                    .detail(tr!("javascript.type_parameter"))
                                    .rank(5),
                            );
                        }
                    }
                }
            }
            node = n.parent();
        }
        for (index, decl) in analysis.visible(from).into_iter().enumerate() {
            let is_type = matches!(
                decl.kind,
                DeclKind::Class
                    | DeclKind::Interface
                    | DeclKind::TypeAlias
                    | DeclKind::Enum
                    | DeclKind::Import
                    | DeclKind::Namespace
            );
            if is_type && seen.insert(decl.name.clone()) {
                items.push(local_item(&analysis, decl, index));
            }
        }
        for item in typescript::builtins::type_items() {
            if seen.insert(item.label.clone()) {
                items.push(item);
            }
        }
        let _ = offset;
        return Some(Completion { from, items });
    }
    None
}

/// The type an object literal is expected to have, from where it is written.
fn expected_type(analysis: &Analysis, object: Node, depth: usize) -> Ty {
    if depth > 6 {
        return Ty::Unknown;
    }
    let at = object.start_byte();
    let Some(parent) = object.parent() else {
        return Ty::Unknown;
    };
    match parent.kind() {
        "variable_declarator" => parent
            .child_by_field_name("type")
            .map_or(Ty::Unknown, |t| analysis.ty_from_type(t, at, 0)),
        "as_expression" | "satisfies_expression" => parent
            .named_child(parent.named_child_count().saturating_sub(1) as u32)
            .filter(|t| t.id() != object.id())
            .map_or(Ty::Unknown, |t| analysis.ty_from_type(t, at, 0)),
        "return_statement" => {
            let mut node = parent.parent();
            while let Some(n) = node {
                if matches!(
                    n.kind(),
                    "function_declaration"
                        | "arrow_function"
                        | "function_expression"
                        | "method_definition"
                ) {
                    return n
                        .child_by_field_name("return_type")
                        .map_or(Ty::Unknown, |t| analysis.ty_from_type(t, at, 0));
                }
                node = n.parent();
            }
            Ty::Unknown
        }
        // A property of an object that has an expected type.
        "pair" => {
            let (Some(key), Some(outer)) = (
                parent.child_by_field_name("key"),
                parent.parent().filter(|o| o.kind() == "object"),
            ) else {
                return Ty::Unknown;
            };
            let name = analysis.text[key.byte_range()]
                .trim_matches(['"', '\''])
                .to_string();
            let outer_ty = expected_type(analysis, outer, depth + 1);
            analysis
                .props_of(&outer_ty, at)
                .into_iter()
                .find(|p| p.name == name)
                .map_or(Ty::Unknown, |p| p.ty)
        }
        // An argument of a call to a function declared here.
        "arguments" => {
            let Some(call) = parent.parent().filter(|c| c.kind() == "call_expression") else {
                return Ty::Unknown;
            };
            let index = {
                let mut cursor = parent.walk();
                parent
                    .named_children(&mut cursor)
                    .position(|a| a.id() == object.id())
            };
            let (Some(callee), Some(index)) = (call.child_by_field_name("function"), index) else {
                return Ty::Unknown;
            };
            let Ty::Function(function) = analysis.ty(callee, at, 0) else {
                return Ty::Unknown;
            };
            let Some(function) = analysis.node_at_range(&function) else {
                return Ty::Unknown;
            };
            let Some(parameters) = function.child_by_field_name("parameters") else {
                return Ty::Unknown;
            };
            let mut cursor = parameters.walk();
            parameters
                .named_children(&mut cursor)
                .nth(index)
                .and_then(|p| p.child_by_field_name("type"))
                .map_or(Ty::Unknown, |t| analysis.ty_from_type(t, at, 0))
        }
        _ => Ty::Unknown,
    }
}

/// In an object literal with a declared type: the properties of that type not given yet.
fn object_key_completion(text: &str, from: usize, end: usize, offset: usize) -> Option<Completion> {
    let (analysis, from) = patched(text, from, end, "")?;
    let leaf = leaf_at(&analysis, from)?;
    if leaf.kind() != "shorthand_property_identifier" {
        return None;
    }
    let object = leaf.parent().filter(|o| o.kind() == "object")?;
    let expected = expected_type(&analysis, object, 0);
    let props = analysis.props_of(&expected, offset);
    if props.is_empty() || matches!(expected, Ty::Object(_)) {
        return None;
    }
    // What the literal already has.
    let mut given: HashSet<String> = HashSet::new();
    let mut cursor = object.walk();
    for member in object.named_children(&mut cursor) {
        let key = match member.kind() {
            "pair" => member.child_by_field_name("key"),
            "method_definition" => member.child_by_field_name("name"),
            "shorthand_property_identifier" if member.id() != leaf.id() => Some(member),
            _ => None,
        };
        if let Some(key) = key {
            given.insert(
                analysis.text[key.byte_range()]
                    .trim_matches(['"', '\''])
                    .to_string(),
            );
        }
    }
    let mut items: Vec<CompletionItem> = Vec::new();
    for (index, prop) in props.into_iter().enumerate() {
        if given.contains(&prop.name) || prop.name == PLACEHOLDER {
            continue;
        }
        let mut item = CompletionItem::new(
            prop.name.clone(),
            if prop.method {
                ItemKind::Method
            } else {
                ItemKind::Property
            },
        )
        .rank(10 + index.min(60) as u32)
        .insert(format!("{}: ", prop.name), None);
        if !prop.detail.is_empty() {
            item = item.detail(prop.detail.clone());
        }
        if !prop.doc.is_empty() {
            item = item.doc(prop.doc.clone());
        }
        items.push(item);
    }
    (!items.is_empty()).then_some(Completion { from, items })
}

fn local_item(analysis: &Analysis, decl: &Decl, order: usize) -> CompletionItem {
    let ty = analysis.decl_ty(decl, 0);
    let kind = match decl.kind {
        DeclKind::Const => ItemKind::Constant,
        DeclKind::Var | DeclKind::Let | DeclKind::Catch => ItemKind::Variable,
        DeclKind::Function => ItemKind::Function,
        DeclKind::Class => ItemKind::Class,
        DeclKind::Param => ItemKind::Parameter,
        DeclKind::Import | DeclKind::Namespace => ItemKind::Module,
        DeclKind::Interface | DeclKind::TypeAlias => ItemKind::Interface,
        DeclKind::Enum => ItemKind::Enum,
    };
    let detail = match &ty {
        Ty::Class(_) => analysis.show(&ty),
        _ => analysis.show(&ty),
    };
    let mut item = CompletionItem::new(decl.name.clone(), kind)
        .detail(detail)
        .rank(10 + order.min(30) as u32);
    if let Some(doc) = analysis.jsdoc_before(&decl.node) {
        item = item.doc(doc);
    }
    if kind == ItemKind::Function {
        let name = &decl.name;
        item = item.insert(format!("{name}()"), Some(name.len() + 1));
    }
    item
}

/// Identifiers used in the document, other than the one under the caret.
fn words(text: &str, skip_from: usize, skip_to: usize) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    let mut start: Option<usize> = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (is_ident(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let word = &text[s..i];
                start = None;
                if s == skip_from && i == skip_to {
                    continue;
                }
                if word.len() > 2
                    && !word.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && !builtins::KEYWORDS.contains(&word)
                    && seen.insert(word)
                {
                    found.push(word.to_string());
                }
            }
            _ => {}
        }
        if found.len() >= 400 {
            break;
        }
    }
    found
}

/// Whether `before` ends with the dot of a member access, not a spread or a decimal number.
fn is_member_dot(before: &str) -> bool {
    let Some(rest) = before.strip_suffix('.') else {
        return false;
    };
    if rest.ends_with('.') {
        return false;
    }
    let word: String = rest
        .chars()
        .rev()
        .take_while(|c| is_ident(*c))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    !word.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn member_completion(text: &str, from: usize, offset: usize) -> Option<Completion> {
    // The code around the caret is usually unfinished (`name.toUp` followed by the next line
    // without a semicolon): the word being typed is replaced by a name the parser accepts.
    let mut end = offset;
    while let Some(c) = text[end..].chars().next() {
        if !is_ident(c) {
            break;
        }
        end += c.len_utf8();
    }
    let source = format!("{}{PLACEHOLDER};{}", &text[..from], &text[end..]);
    let source: &str = &source;
    let analysis = Analysis::new(source)?;
    let leaf = analysis.root().descendant_for_byte_range(from, from + 1)?;
    let member = if leaf.kind() == "property_identifier" {
        leaf.parent().filter(|p| p.kind() == "member_expression")
    } else {
        None
    };
    let Some(member) = member else {
        return Some(Completion {
            from,
            items: words_as_members(text, from, offset),
        });
    };
    let object = member.child_by_field_name("object")?;
    let ty = analysis.ty_of(object, offset);
    let props = analysis.props_of(&ty, offset);
    if props.is_empty() {
        return Some(Completion {
            from,
            items: words_as_members(text, from, offset),
        });
    }
    let call_follows = text[offset..].starts_with('(');
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen = HashSet::new();
    for (index, prop) in props.into_iter().enumerate() {
        if prop.name == PLACEHOLDER || !seen.insert(prop.name.clone()) {
            continue;
        }
        items.push(prop_item(&prop, index, call_follows));
    }
    Some(Completion { from, items })
}

fn prop_item(prop: &Prop, index: usize, call_follows: bool) -> CompletionItem {
    let kind = if prop.method {
        ItemKind::Method
    } else {
        ItemKind::Property
    };
    // Own members first, then those every object has.
    let mut item = CompletionItem::new(prop.name.clone(), kind).rank(10 + index.min(60) as u32);
    if !prop.detail.is_empty() {
        item = item.detail(prop.detail.clone());
    }
    if !prop.doc.is_empty() {
        item = item.doc(prop.doc.clone());
    }
    if prop.method && !call_follows {
        let name = &prop.name;
        // The caret goes between the parentheses, unless the method takes nothing.
        let caret = if prop.detail.starts_with("()") {
            name.len() + 2
        } else {
            name.len() + 1
        };
        item = item.insert(format!("{name}()"), Some(caret));
    }
    item
}

fn words_as_members(text: &str, from: usize, offset: usize) -> Vec<CompletionItem> {
    words(text, from, offset)
        .into_iter()
        .map(|w| CompletionItem::new(w, ItemKind::Text).rank(90))
        .collect()
}

/// Inside a comment: nothing. Inside the module string of an import or require: paths.
/// None when the position is ordinary code.
fn in_string_or_comment(
    text: &str,
    offset: usize,
    path: Option<&Path>,
) -> Option<Option<Completion>> {
    let analysis = Analysis::new(text)?;
    let leaf = analysis
        .root()
        .descendant_for_byte_range(offset.saturating_sub(1).min(offset), offset)?;
    let mut node = Some(leaf);
    while let Some(n) = node {
        match n.kind() {
            "comment" | "regex" | "regex_pattern" => return Some(None),
            "template_string" => {
                // Inside `${…}` it is code again.
                let mut inner = leaf;
                loop {
                    if inner.kind() == "template_substitution" {
                        return None;
                    }
                    match inner.parent() {
                        Some(parent) if parent.id() != n.id() => inner = parent,
                        _ => break,
                    }
                }
                return Some(None);
            }
            "string" => {
                let start = n.start_byte() + 1;
                if offset < start || offset > n.end_byte() {
                    return None;
                }
                let is_module = match n.parent() {
                    Some(p) if matches!(p.kind(), "import_statement" | "export_statement") => true,
                    Some(p) if p.kind() == "arguments" => p
                        .parent()
                        .and_then(|call| call.child_by_field_name("function"))
                        .is_some_and(|f| matches!(&text[f.byte_range()], "require" | "import")),
                    _ => false,
                };
                if !is_module {
                    return Some(None);
                }
                return Some(module_paths(&text[start..offset], start, path));
            }
            _ => node = n.parent(),
        }
    }
    None
}

pub const NODE_MODULES: &[&str] = &[
    "assert",
    "buffer",
    "child_process",
    "cluster",
    "crypto",
    "dns",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "querystring",
    "readline",
    "stream",
    "string_decoder",
    "timers",
    "tls",
    "url",
    "util",
    "v8",
    "vm",
    "worker_threads",
    "zlib",
];

/// Files, folders and packages for the module string `typed` (what is between the quote and
/// the caret) that starts at byte `start`.
fn module_paths(typed: &str, start: usize, path: Option<&Path>) -> Option<Completion> {
    let slash = typed.rfind('/');
    let from = start + slash.map_or(0, |s| s + 1);
    let mut items: Vec<CompletionItem> = Vec::new();
    let base = path.and_then(Path::parent);
    if typed.starts_with('.') {
        let base = base?;
        let dir_part = slash.map_or("", |s| &typed[..=s]);
        if slash.is_none() {
            return Some(Completion {
                from,
                items: vec![
                    CompletionItem::new("./", ItemKind::Folder).rank(1),
                    CompletionItem::new("../", ItemKind::Folder).rank(2),
                ],
            });
        }
        items = list_dir(&base.join(dir_part), true);
    } else {
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut dir = base.map(Path::to_path_buf);
        while let Some(d) = dir {
            roots.push(d.join("node_modules"));
            dir = d.parent().map(Path::to_path_buf);
        }
        match slash {
            Some(s) => {
                // `@scope/…` or `package/sub/path`
                let package_dir = &typed[..=s];
                for root in &roots {
                    items.extend(list_dir(&root.join(package_dir), false));
                }
            }
            None => {
                for root in &roots {
                    for item in list_dir(root, false) {
                        if item.label.starts_with('.') {
                            continue;
                        }
                        // A package is imported by its name; a scope is a folder of packages.
                        if item.label.starts_with('@') {
                            items.push(item);
                        } else {
                            let name = item.label.trim_end_matches('/').to_string();
                            items.push(
                                CompletionItem::new(name, ItemKind::Module)
                                    .detail("node_modules")
                                    .rank(6),
                            );
                        }
                    }
                }
                for name in package_dependencies(base) {
                    items.push(
                        CompletionItem::new(name, ItemKind::Module)
                            .detail("package.json")
                            .rank(5),
                    );
                }
                for name in NODE_MODULES {
                    items.push(
                        CompletionItem::new(*name, ItemKind::Module)
                            .detail("Node.js")
                            .rank(20),
                    );
                    items.push(
                        CompletionItem::new(format!("node:{name}"), ItemKind::Module)
                            .detail("Node.js")
                            .rank(21),
                    );
                }
            }
        }
    }
    let mut seen = HashSet::new();
    items.retain(|i| seen.insert(i.label.clone()));
    Some(Completion { from, items })
}

fn list_dir(dir: &Path, only_modules: bool) -> Vec<CompletionItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten().take(2000) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir()) || entry.path().is_dir();
        if is_dir {
            items.push(
                CompletionItem::new(format!("{name}/"), ItemKind::Folder)
                    .insert(format!("{name}/"), None)
                    .rank(10),
            );
        } else {
            let wanted = !only_modules
                || Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| {
                        matches!(
                            e.to_ascii_lowercase().as_str(),
                            "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "json" | "node"
                        )
                    });
            if wanted && !name.starts_with('.') {
                items.push(CompletionItem::new(name, ItemKind::File).rank(11));
            }
        }
    }
    items
}

/// Names in the dependencies of the closest package.json.
pub fn package_dependencies(base: Option<&Path>) -> Vec<String> {
    let mut dir = base.map(Path::to_path_buf);
    while let Some(d) = dir {
        if let Ok(text) = std::fs::read_to_string(d.join("package.json"))
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
        {
            let mut names = Vec::new();
            for key in [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ] {
                if let Some(map) = json.get(key).and_then(|v| v.as_object()) {
                    names.extend(map.keys().cloned());
                }
            }
            return names;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    Vec::new()
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/complete/tests.rs"]
mod tests;
