//! Going to where a name is declared and finding where it is used, from the syntax tree: the
//! local answer when no language server is there to give a better one.

use super::analysis::{Analysis, DeclKind, Ty, dialect};
use crate::languages::Location;
use std::{
    ops::Range,
    path::{Path, PathBuf},
};
use tree_sitter::Node;

const MAX_TEXT: usize = 1_000_000;
const EXTENSIONS: [&str; 9] = ["js", "mjs", "cjs", "jsx", "json", "ts", "tsx", "mts", "cts"];

fn name_node(analysis: &Analysis, offset: usize) -> Option<Node<'_>> {
    let at = analysis.root().descendant_for_byte_range(offset, offset)?;
    let is_name = |n: &Node| {
        matches!(
            n.kind(),
            "identifier"
                | "property_identifier"
                | "shorthand_property_identifier"
                | "shorthand_property_identifier_pattern"
                | "type_identifier"
        )
    };
    if is_name(&at) {
        return Some(at);
    }
    // The pointer may be just past the last character of the word.
    let before = analysis
        .root()
        .descendant_for_byte_range(offset.saturating_sub(1), offset.saturating_sub(1))?;
    is_name(&before).then_some(before)
}

/// The file a module specifier stands for, as Node resolves it: a relative path, or a package
/// of the closest `node_modules`.
pub fn resolve_module(from_dir: &Path, source: &str) -> Option<PathBuf> {
    if source.starts_with('.') {
        resolve_file(&from_dir.join(source))
    } else {
        // `paths` and `baseUrl` of the tsconfig.json or jsconfig.json come first.
        resolve_alias(from_dir, source).or_else(|| resolve_package(from_dir, source))
    }
}

/// The TypeScript file a JavaScript specifier names: `./a.js` is written for `a.ts`.
fn typescript_twin(base: &Path) -> Vec<PathBuf> {
    let swaps: &[(&str, &[&str])] = &[
        ("js", &["ts", "tsx"]),
        ("jsx", &["tsx"]),
        ("mjs", &["mts"]),
        ("cjs", &["cts"]),
    ];
    let Some(extension) = base.extension().and_then(|e| e.to_str()) else {
        return Vec::new();
    };
    swaps
        .iter()
        .filter(|(from, _)| *from == extension)
        .flat_map(|(_, to)| to.iter().map(|t| base.with_extension(t)))
        .collect()
}

/// Reads a tsconfig.json: comments and trailing commas are allowed in it.
fn read_config(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut clean = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let (mut i, mut in_string) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            clean.push(c);
            if c == '\\' && i + 1 < chars.len() {
                clean.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            clean.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            clean.push(c);
        }
        i += 1;
    }
    // A comma before a closing bracket.
    let mut out = String::with_capacity(clean.len());
    let cleaned: Vec<char> = clean.chars().collect();
    let mut in_string = false;
    for (index, c) in cleaned.iter().enumerate() {
        if *c == '"' && (index == 0 || cleaned[index - 1] != '\\') {
            in_string = !in_string;
        }
        if !in_string
            && *c == ','
            && cleaned[index + 1..]
                .iter()
                .find(|n| !n.is_whitespace())
                .is_some_and(|n| matches!(n, '}' | ']'))
        {
            continue;
        }
        out.push(*c);
    }
    serde_json::from_str(&out).ok()
}

/// The `baseUrl` and `paths` of the closest configuration, following `extends`.
/// The `paths` of a `tsconfig.json` or `jsconfig.json`: its folder, and (alias, targets).
type CompilerPaths = (PathBuf, Vec<(String, Vec<String>)>);

fn compiler_paths(from_dir: &Path) -> Option<CompilerPaths> {
    let mut dir = Some(from_dir.to_path_buf());
    while let Some(d) = dir {
        for name in ["tsconfig.json", "jsconfig.json"] {
            let file = d.join(name);
            if file.is_file() {
                return paths_of(&file, 0);
            }
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

fn paths_of(file: &Path, depth: usize) -> Option<CompilerPaths> {
    let config = read_config(file)?;
    let folder = file.parent()?.to_path_buf();
    let options = &config["compilerOptions"];
    let base = options["baseUrl"].as_str().map(|b| folder.join(b));
    let paths: Vec<(String, Vec<String>)> = options["paths"]
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(pattern, targets)| {
                    let targets = targets
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|t| t.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    (pattern.clone(), targets)
                })
                .collect()
        })
        .unwrap_or_default();
    if base.is_some() || !paths.is_empty() {
        // Without baseUrl, the targets are relative to the configuration itself.
        return Some((base.unwrap_or(folder), paths));
    }
    // Settings come from the configuration this one extends.
    if depth < 5
        && let Some(parent) = config["extends"].as_str().filter(|e| e.starts_with('.'))
    {
        let mut next = folder.join(parent);
        if !next.is_file() {
            let mut with = next.into_os_string();
            with.push(".json");
            next = PathBuf::from(with);
        }
        return paths_of(&next, depth + 1);
    }
    None
}

/// A specifier written with an alias of the configuration: `@/lib/x`, or relative to `baseUrl`.
fn resolve_alias(from_dir: &Path, source: &str) -> Option<PathBuf> {
    let (base, paths) = compiler_paths(from_dir)?;
    for (pattern, targets) in &paths {
        let captured = match pattern.split_once('*') {
            Some((prefix, suffix)) => source
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .map(str::to_string),
            None => (pattern == source).then(String::new),
        };
        let Some(captured) = captured else { continue };
        for target in targets {
            let path = target.replacen('*', &captured, 1);
            if let Some(file) = resolve_file(&base.join(path)) {
                // `./` in the middle of the path goes away.
                return Some(file.components().collect());
            }
        }
    }
    // A bare specifier can also be a path from `baseUrl`.
    resolve_file(&base.join(source)).filter(|_| paths_have_base(from_dir))
}

fn paths_have_base(from_dir: &Path) -> bool {
    let mut dir = Some(from_dir.to_path_buf());
    while let Some(d) = dir {
        for name in ["tsconfig.json", "jsconfig.json"] {
            let file = d.join(name);
            if file.is_file() {
                return read_config(&file)
                    .is_some_and(|c| c["compilerOptions"]["baseUrl"].is_string());
            }
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    false
}

/// A path as a file, with the extensions and `index` files Node tries.
fn resolve_file(base: &Path) -> Option<PathBuf> {
    let mut candidates = vec![base.to_path_buf()];
    for extension in EXTENSIONS {
        let mut with = base.to_path_buf().into_os_string();
        with.push(format!(".{extension}"));
        candidates.push(PathBuf::from(with));
    }
    for extension in EXTENSIONS {
        candidates.push(base.join(format!("index.{extension}")));
    }
    // Declaration files, for the types of a JavaScript module.
    let mut declaration = base.to_path_buf().into_os_string();
    declaration.push(".d.ts");
    candidates.push(PathBuf::from(declaration));
    candidates.push(base.join("index.d.ts"));
    // `./a.js` written for `a.ts`
    candidates.extend(typescript_twin(base));
    candidates.into_iter().find(|c| c.is_file())
}

/// The entry point a `package.json` names: `exports` for the root, then `module`, `main`.
fn package_entry(package: &serde_json::Value) -> Option<String> {
    fn condition(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(path) => Some(path.clone()),
            serde_json::Value::Object(map) => {
                ["types", "import", "module", "node", "require", "default"]
                    .iter()
                    .filter(|key| **key != "types" || dialect().is_ts())
                    .find_map(|key| map.get(*key).and_then(condition))
            }
            _ => None,
        }
    }
    package
        .get("exports")
        .and_then(|e| match e {
            serde_json::Value::Object(map) if map.contains_key(".") => {
                map.get(".").and_then(condition)
            }
            other => condition(other),
        })
        .or_else(|| {
            // TypeScript wants the declarations of a package, when it has them.
            dialect()
                .is_ts()
                .then(|| {
                    package
                        .get("types")
                        .or_else(|| package.get("typings"))
                        .and_then(|m| m.as_str())
                        .map(String::from)
                })
                .flatten()
        })
        .or_else(|| {
            package
                .get("module")
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .or_else(|| {
            package
                .get("main")
                .and_then(|m| m.as_str())
                .map(String::from)
        })
}

/// `express`, `@scope/name/sub/path`: the file inside the closest `node_modules` holding it.
fn resolve_package(from_dir: &Path, source: &str) -> Option<PathBuf> {
    if source.starts_with("node:") {
        return None;
    }
    let mut parts = source.split('/');
    let first = parts.next()?;
    let name = if first.starts_with('@') {
        format!("{first}/{}", parts.next()?)
    } else {
        first.to_string()
    };
    let subpath: Vec<&str> = parts.collect();
    let mut dir = Some(from_dir.to_path_buf());
    while let Some(d) = dir {
        let modules = d.join("node_modules");
        let mut found = package_file(&modules.join(&name), &subpath);
        if found.is_none() && dialect().is_ts() {
            // `@scope/name` is typed by `@types/scope__name`.
            let typed = name.trim_start_matches('@').replace('/', "__");
            found = package_file(&modules.join("@types").join(typed), &subpath);
        }
        if found.is_some() {
            return found;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

/// A file of a package folder: the entry point, or the sub-path asked for.
fn package_file(package: &Path, subpath: &[&str]) -> Option<PathBuf> {
    if !package.is_dir() {
        return None;
    }
    if !subpath.is_empty() {
        return resolve_file(&package.join(subpath.join("/")));
    }
    std::fs::read_to_string(package.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| package_entry(&json))
        .and_then(|e| resolve_file(&package.join(e.trim_start_matches("./"))))
        .or_else(|| resolve_file(&package.join("index")))
}

/// The export `imported` of the module in `file`, or the start of the file.
fn export_location(file: &Path, imported: &str) -> Option<Location> {
    let text = std::fs::read_to_string(file)
        .ok()
        .filter(|t| t.len() <= MAX_TEXT)?;
    let range = Analysis::new(&text)
        .and_then(|analysis| export_range(&analysis, imported))
        .unwrap_or(0..0);
    Some(Location::in_text(Some(file.to_path_buf()), &text, range))
}

fn export_range(analysis: &Analysis, imported: &str) -> Option<Range<usize>> {
    if imported == "default" {
        let mut cursor = analysis.root().walk();
        for statement in analysis.root().named_children(&mut cursor) {
            if statement.kind() == "export_statement"
                && analysis.text[statement.byte_range()].contains("default")
            {
                // `export default function name()` points at the name, otherwise at the keyword.
                if let Some(name) = statement
                    .child_by_field_name("declaration")
                    .and_then(|d| d.child_by_field_name("name"))
                {
                    return Some(name.byte_range());
                }
                return Some(
                    statement.start_byte()..statement.start_byte() + "export default".len(),
                );
            }
        }
        return None;
    }
    if imported == "*" {
        return None;
    }
    analysis
        .scopes
        .first()?
        .decls
        .iter()
        .find(|d| d.name == imported && d.kind != DeclKind::Import)
        .map(|d| d.name_range.clone())
}

/// Where the name at `offset` is declared. Empty when it is itself a declaration, or unknown.
pub fn definition(text: &str, offset: usize, path: Option<&Path>) -> Vec<Location> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let Some(analysis) = Analysis::new(text) else {
        return Vec::new();
    };
    let Some(node) = name_node(&analysis, offset) else {
        return Vec::new();
    };
    let name = &text[node.byte_range()];
    match node.kind() {
        "identifier"
        | "shorthand_property_identifier"
        | "shorthand_property_identifier_pattern"
        | "type_identifier" => {
            let Some(decl) = analysis.resolve(name, node.start_byte()) else {
                return Vec::new();
            };
            if decl.name_range == node.byte_range() {
                return Vec::new();
            }
            if decl.kind == DeclKind::Import
                && let Some((source, imported)) = &decl.import
                && let Some(dir) = path.and_then(Path::parent)
                && let Some(file) = resolve_module(dir, source)
                && let Some(location) = export_location(&file, imported)
            {
                return vec![location];
            }
            vec![Location::in_text(None, text, decl.name_range.clone())]
        }
        "property_identifier" => {
            let Some(member) = node.parent().filter(|p| p.kind() == "member_expression") else {
                return Vec::new();
            };
            let Some(object) = member.child_by_field_name("object") else {
                return Vec::new();
            };
            let ty = analysis.ty_of(object, node.start_byte());
            // A required module: its file.
            if let (Ty::Module(source), Some(dir)) = (&ty, path.and_then(Path::parent))
                && let Some(file) = resolve_module(dir, source)
                && let Some(location) = export_location(&file, name)
            {
                return vec![location];
            }
            analysis
                .props_of(&ty, node.start_byte())
                .into_iter()
                .find(|p| p.name == name)
                .and_then(|p| p.def)
                .filter(|def| *def != node.byte_range())
                .map(|def| vec![Location::in_text(None, text, def)])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Where the type of the name at `offset` is declared: the class of a value.
pub fn type_definition(text: &str, offset: usize) -> Vec<Location> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let Some(analysis) = Analysis::new(text) else {
        return Vec::new();
    };
    let Some(node) = name_node(&analysis, offset) else {
        return Vec::new();
    };
    let ty = match node.kind() {
        "property_identifier" => node
            .parent()
            .filter(|p| p.kind() == "member_expression")
            .map_or(Ty::Unknown, |m| analysis.ty_of(m, node.start_byte())),
        _ => analysis.ty_of(node, node.start_byte()),
    };
    let (Ty::Instance(range) | Ty::Class(range) | Ty::Interface(range) | Ty::Enum(range)) = ty
    else {
        return Vec::new();
    };
    let Some(class) = analysis.node_at_range(&range) else {
        return Vec::new();
    };
    let name = class
        .child_by_field_name("name")
        .map_or(class.byte_range(), |n| n.byte_range());
    vec![Location::in_text(None, text, name)]
}

/// Every place of the document that is the same name as the one at `offset`, declaration
/// included.
pub fn highlights(text: &str, offset: usize) -> Vec<Range<usize>> {
    let mut places: Vec<Range<usize>> = references(text, offset)
        .into_iter()
        .map(|l| l.range_in(text))
        .collect();
    if let Some(analysis) = Analysis::new(text)
        && let Some(node) = name_node(&analysis, offset)
    {
        let name = &text[node.byte_range()];
        let declaration = match node.kind() {
            "property_identifier" => definition_of_member(&analysis, node),
            _ => analysis
                .resolve(name, node.start_byte())
                .map(|d| d.name_range.clone()),
        };
        // The declaration is a place too, and so is the name under the caret itself.
        places.extend(declaration);
        if places
            .iter()
            .any(|p| p.start <= node.start_byte() && node.end_byte() <= p.end)
            || !places.is_empty()
        {
            places.push(node.byte_range());
        }
    }
    places.sort_by_key(|r| r.start);
    places.dedup();
    places
}

/// Where the name at `offset` is used in the document, its declaration left out.
pub fn references(text: &str, offset: usize) -> Vec<Location> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let Some(analysis) = Analysis::new(text) else {
        return Vec::new();
    };
    let Some(node) = name_node(&analysis, offset) else {
        return Vec::new();
    };
    let name = text[node.byte_range()].to_string();
    let mut found: Vec<Range<usize>> = Vec::new();
    if node.kind() == "property_identifier" {
        // A member: the places that name the same declared member.
        let target = definition_of_member(&analysis, node).unwrap_or(node.byte_range());
        visit(analysis.root(), &mut |n| {
            if n.kind() == "property_identifier" && text[n.byte_range()] == name {
                let same = definition_of_member(&analysis, n).unwrap_or(n.byte_range());
                if same == target && n.byte_range() != target {
                    found.push(n.byte_range());
                }
            }
        });
    } else {
        let Some(decl) = analysis.resolve(&name, node.start_byte()) else {
            return Vec::new();
        };
        let target = decl.name_range.clone();
        visit(analysis.root(), &mut |n| {
            if matches!(
                n.kind(),
                "identifier"
                    | "shorthand_property_identifier"
                    | "shorthand_property_identifier_pattern"
                    | "type_identifier"
            ) && text[n.byte_range()] == name
                && n.byte_range() != target
                && analysis
                    .resolve(&name, n.start_byte())
                    .is_some_and(|d| d.name_range == target)
            {
                found.push(n.byte_range());
            }
        });
    }
    found.sort_by_key(|r| r.start);
    found.dedup();
    found
        .into_iter()
        .take(500)
        .map(|r| Location::in_text(None, text, r))
        .collect()
}

/// Where the member named by `property` (in `a.b` or at its declaration) is declared.
fn definition_of_member(analysis: &Analysis, property: Node) -> Option<Range<usize>> {
    let parent = property.parent()?;
    match parent.kind() {
        "member_expression" => {
            let object = parent.child_by_field_name("object")?;
            let ty = analysis.ty_of(object, property.start_byte());
            analysis
                .props_of(&ty, property.start_byte())
                .into_iter()
                .find(|p| p.name == analysis.text[property.byte_range()])
                .and_then(|p| p.def)
        }
        "method_definition"
        | "field_definition"
        | "pair"
        | "public_field_definition"
        | "property_signature"
        | "method_signature"
        | "abstract_method_signature"
        | "enum_assignment" => Some(property.byte_range()),
        _ => None,
    }
}

fn visit<'t>(node: Node<'t>, f: &mut impl FnMut(Node<'t>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, f);
    }
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/navigate/tests.rs"]
mod tests;
