//! "Import this name" fixes (Alt+Enter on a name that is not declared): the exports of the
//! other files of the project, the modules of Node.js and the dependencies of `package.json`.

use super::{
    analysis::{Analysis, dialect},
    builtins,
};
use crate::languages::{QuickFix, TextEdit};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};
use tree_sitter::Node;

const MAX_FILES: usize = 600;
const MAX_FILE_SIZE: u64 = 300_000;
const SKIPPED: [&str; 9] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    ".next",
    ".atelier",
];
const EXTENSIONS: [&str; 4] = ["js", "mjs", "cjs", "jsx"];
const TYPESCRIPT_EXTENSIONS: [&str; 4] = ["ts", "tsx", "mts", "cts"];

/// What a file exports.
#[derive(Default, Debug, PartialEq)]
pub struct Exports {
    pub named: Vec<String>,
    /// The file has a default export, with the name it was declared with.
    pub default: Option<Option<String>>,
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn pattern_names(node: Node, src: &str, out: &mut Vec<String>) {
    match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            out.push(text(node, src).to_string())
        }
        "assignment_pattern" | "object_assignment_pattern" => {
            if let Some(left) = node.child_by_field_name("left") {
                pattern_names(left, src, out);
            }
        }
        "pair_pattern" => {
            if let Some(value) = node.child_by_field_name("value") {
                pattern_names(value, src, out);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                pattern_names(child, src, out);
            }
        }
    }
}

/// The exports of a JavaScript module, ES or CommonJS.
pub fn exports_of(source: &str) -> Exports {
    let Some(analysis) = Analysis::new(source) else {
        return Exports::default();
    };
    let mut exports = Exports::default();
    let root = analysis.root();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        match statement.kind() {
            "export_statement" => export_statement(statement, source, &mut exports),
            "expression_statement" => {
                if let Some(assignment) = statement
                    .named_child(0)
                    .filter(|n| n.kind() == "assignment_expression")
                {
                    commonjs_assignment(assignment, source, &mut exports);
                }
            }
            _ => {}
        }
    }
    exports.named.sort();
    exports.named.dedup();
    exports
}

fn export_statement(statement: Node, src: &str, exports: &mut Exports) {
    let is_default = {
        let mut cursor = statement.walk();
        statement
            .children(&mut cursor)
            .any(|c| !c.is_named() && text(c, src) == "default")
    };
    let declaration = statement.child_by_field_name("declaration");
    if is_default {
        let name = declaration
            .and_then(|d| d.child_by_field_name("name"))
            .map(|n| text(n, src).to_string());
        exports.default = Some(name);
        return;
    }
    if let Some(declaration) = declaration {
        match declaration.kind() {
            "lexical_declaration" | "variable_declaration" => {
                let mut cursor = declaration.walk();
                for declarator in declaration.named_children(&mut cursor) {
                    if let Some(name) = declarator.child_by_field_name("name") {
                        pattern_names(name, src, &mut exports.named);
                    }
                }
            }
            _ => {
                if let Some(name) = declaration.child_by_field_name("name") {
                    exports.named.push(text(name, src).to_string());
                }
            }
        }
        return;
    }
    let mut cursor = statement.walk();
    for clause in statement
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "export_clause")
    {
        let mut inner = clause.walk();
        for specifier in clause.named_children(&mut inner) {
            let exported = specifier
                .child_by_field_name("alias")
                .or_else(|| specifier.child_by_field_name("name"));
            if let Some(exported) = exported {
                let name = text(exported, src);
                if name == "default" {
                    exports.default = Some(None);
                } else {
                    exports.named.push(name.to_string());
                }
            }
        }
    }
}

fn commonjs_assignment(assignment: Node, src: &str, exports: &mut Exports) {
    let (Some(left), Some(right)) = (
        assignment.child_by_field_name("left"),
        assignment.child_by_field_name("right"),
    ) else {
        return;
    };
    let left_text = text(left, src);
    if left_text == "module.exports" {
        if right.kind() == "object" {
            let mut cursor = right.walk();
            for member in right.named_children(&mut cursor) {
                let key = match member.kind() {
                    "pair" => member.child_by_field_name("key"),
                    "method_definition" => member.child_by_field_name("name"),
                    "shorthand_property_identifier" => Some(member),
                    _ => None,
                };
                if let Some(key) = key {
                    exports
                        .named
                        .push(text(key, src).trim_matches(['"', '\'']).to_string());
                }
            }
        } else {
            let name = (right.kind() == "identifier").then(|| text(right, src).to_string());
            exports.default = Some(name);
        }
    } else if let Some(name) = left_text
        .strip_prefix("module.exports.")
        .or_else(|| left_text.strip_prefix("exports."))
        && !name.contains('.')
    {
        exports.named.push(name.to_string());
    }
}

fn is_module_style(source: &str) -> bool {
    // ES modules unless the file visibly uses `require` and no `import`.
    let uses_import = source
        .lines()
        .any(|l| l.trim_start().starts_with("import ") || l.trim_start().starts_with("export "));
    let uses_require = source.contains("require(") || source.contains("module.exports");
    uses_import || !uses_require
}

fn camel_case(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.' | ' ') {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The specifier of `target` seen from the folder `from_dir`, with a leading `./`.
fn relative_specifier(from_dir: &Path, target: &Path, drop_extension: bool) -> Option<String> {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from.len() {
        parts.push("..".into());
    }
    for component in &to[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    let mut specifier = parts.join("/");
    if drop_extension
        && let Some(dot) = specifier.rfind('.')
        && specifier[dot..].len() <= 5
    {
        specifier.truncate(dot);
    }
    if !specifier.starts_with("..") {
        specifier.insert_str(0, "./");
    }
    Some(specifier)
}

fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !SKIPPED.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                EXTENSIONS.contains(&e) || (dialect().is_ts() && TYPESCRIPT_EXTENSIONS.contains(&e))
            }) && !path.to_string_lossy().ends_with(".d.ts")
                && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_SIZE)
            {
                files.push(path);
                if files.len() >= MAX_FILES {
                    return files;
                }
            }
        }
    }
    files
}

/// Where a new import goes: after the last import (or the last `require` line), after a
/// shebang or a `'use strict'`, otherwise at the top. The text to insert includes its line break.
fn insertion_point(analysis: &Analysis, module_style: bool) -> (usize, bool) {
    let root = analysis.root();
    let mut cursor = root.walk();
    let mut last: Option<usize> = None;
    for statement in root.named_children(&mut cursor) {
        let is_import = if module_style {
            statement.kind() == "import_statement"
        } else {
            matches!(
                statement.kind(),
                "lexical_declaration" | "variable_declaration"
            ) && text(statement, &analysis.text).contains("require(")
        };
        let is_directive = statement.kind() == "expression_statement"
            && matches!(
                text(statement, &analysis.text).trim_matches(';'),
                "'use strict'" | "\"use strict\""
            );
        let is_shebang = statement.kind() == "hash_bang_line";
        if is_import || is_directive || is_shebang {
            last = Some(statement.end_byte());
        } else if last.is_some() && !is_import {
            break;
        }
    }
    match last {
        Some(end) => (end, true),
        None => (0, false),
    }
}

/// An import statement for `name` taken from `source`: a new line, or the name added to an
/// existing import of the same module.
fn import_edit(
    analysis: &Analysis,
    name: &str,
    source: &str,
    default: bool,
    module_style: bool,
) -> Option<(String, Vec<TextEdit>)> {
    let src = analysis.text.as_str();
    if module_style {
        // Another import of the same module: add the name to it.
        let root = analysis.root();
        let mut cursor = root.walk();
        for statement in root
            .named_children(&mut cursor)
            .filter(|s| s.kind() == "import_statement")
        {
            let same = statement
                .child_by_field_name("source")
                .is_some_and(|s| text(s, src).trim_matches(['"', '\'']) == source);
            if !same {
                continue;
            }
            let clause = {
                let mut inner = statement.walk();
                statement
                    .named_children(&mut inner)
                    .find(|c| c.kind() == "import_clause")
            };
            if !default
                && let Some(clause) = clause
                && let Some(named) = {
                    let mut inner = clause.walk();
                    clause
                        .named_children(&mut inner)
                        .find(|c| c.kind() == "named_imports")
                }
            {
                let closing = named.end_byte() - 1;
                let before = src[named.start_byte() + 1..closing].trim_end();
                let separator = if before.trim().is_empty() { " " } else { ", " };
                let at = named.start_byte() + 1 + before.len();
                let _ = separator;
                let insert = if before.trim().is_empty() {
                    format!(" {name} ")
                } else {
                    format!(", {name}")
                };
                return Some((
                    trf!("javascript.add_import", name = name, source = source),
                    vec![TextEdit {
                        range: at..at,
                        new_text: insert,
                    }],
                ));
            }
        }
        let statement = if default {
            format!("import {name} from '{source}';")
        } else {
            format!("import {{ {name} }} from '{source}';")
        };
        let (point, after) = insertion_point(analysis, true);
        let new_text = if after {
            format!("\n{statement}")
        } else {
            format!("{statement}\n")
        };
        return Some((
            format!("Importer « {name} » depuis « {source} »"),
            vec![TextEdit {
                range: point..point,
                new_text,
            }],
        ));
    }
    let statement = if default {
        format!("const {name} = require('{source}');")
    } else {
        format!("const {{ {name} }} = require('{source}');")
    };
    let (point, after) = insertion_point(analysis, false);
    let new_text = if after {
        format!("\n{statement}")
    } else {
        format!("{statement}\n")
    };
    Some((
        format!("Importer « {name} » depuis « {source} » (require)"),
        vec![TextEdit {
            range: point..point,
            new_text,
        }],
    ))
}

/// The name that is not declared at `offset`, if there is one.
fn undeclared_name(analysis: &Analysis, offset: usize) -> Option<(String, Range<usize>)> {
    let root = analysis.root();
    let at = root.descendant_for_byte_range(offset, offset)?;
    let is_name = |n: &Node| n.kind() == "identifier" || n.kind() == "type_identifier";
    let node = if is_name(&at) {
        at
    } else {
        let before =
            root.descendant_for_byte_range(offset.saturating_sub(1), offset.saturating_sub(1))?;
        is_name(&before).then_some(before)?
    };
    // A property (`a.b`) or a key is not a free name.
    let name = text(node, &analysis.text).to_string();
    if analysis.resolve(&name, node.start_byte()).is_some() {
        return None;
    }
    if builtins::global_doc(&name).is_some() || builtins::KEYWORDS.contains(&name.as_str()) {
        return None;
    }
    if crate::languages::typescript::builtins::is_library_type(&name) {
        return None;
    }
    Some((name, node.byte_range()))
}

/// The fixes that import the undeclared name at `offset`.
pub fn import_fixes(
    source: &str,
    offset: usize,
    path: Option<&Path>,
    root: &Path,
) -> Vec<QuickFix> {
    let Some(analysis) = Analysis::new(source) else {
        return Vec::new();
    };
    let Some((name, _)) = undeclared_name(&analysis, offset) else {
        return Vec::new();
    };
    let module_style = is_module_style(source);
    // Existing imports without extension: the same style for new ones.
    let drop_extension = analysis
        .root()
        .named_children(&mut analysis.root().walk())
        .filter(|s| s.kind() == "import_statement")
        .filter_map(|s| s.child_by_field_name("source"))
        .map(|s| text(s, source).trim_matches(['"', '\'']).to_string())
        .any(|s| s.starts_with('.') && Path::new(&s).extension().is_none());
    let mut fixes: Vec<QuickFix> = Vec::new();
    let mut add = |entry: Option<(String, Vec<TextEdit>)>| {
        if let Some((title, edits)) = entry
            && !fixes.iter().any(|f| f.title == title)
        {
            fixes.push(QuickFix { title, edits });
        }
    };

    // The other files of the project.
    if let Some(dir) = path.and_then(Path::parent) {
        for file in project_files(root) {
            if path.is_some_and(|p| p == file) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            // Cheap filter: the name must appear in the file at all.
            if !content.contains(name.as_str())
                && file
                    .file_stem()
                    .is_none_or(|s| camel_case(&s.to_string_lossy()) != name)
            {
                continue;
            }
            let exports = exports_of(&content);
            let stem = file
                .file_stem()
                .map(|s| camel_case(&s.to_string_lossy()))
                .unwrap_or_default();
            // TypeScript files are imported without their extension.
            let typescript_file = file
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| TYPESCRIPT_EXTENSIONS.contains(&e));
            let Some(specifier) = relative_specifier(dir, &file, drop_extension || typescript_file)
            else {
                continue;
            };
            if exports.named.contains(&name) {
                add(import_edit(
                    &analysis,
                    &name,
                    &specifier,
                    false,
                    module_style,
                ));
            }
            match &exports.default {
                Some(Some(declared)) if *declared == name => add(import_edit(
                    &analysis,
                    &name,
                    &specifier,
                    true,
                    module_style,
                )),
                Some(None) if stem == name => add(import_edit(
                    &analysis,
                    &name,
                    &specifier,
                    true,
                    module_style,
                )),
                _ => {}
            }
        }
    }

    // Node.js: a module by its name, or a function of a well-known module.
    if super::complete::NODE_MODULES.contains(&name.as_str()) {
        add(import_edit(&analysis, &name, &name, true, module_style));
    }
    for module in ["fs", "path", "os"] {
        if super::analysis::module_member_names(module).contains(&name) {
            add(import_edit(&analysis, &name, module, false, module_style));
        }
    }

    // A dependency of the project.
    for dependency in super::complete::package_dependencies(path.and_then(Path::parent)) {
        let short = dependency
            .rsplit('/')
            .next()
            .unwrap_or(&dependency)
            .to_string();
        if short == name || camel_case(&short) == name {
            add(import_edit(
                &analysis,
                &name,
                &dependency,
                true,
                module_style,
            ));
        }
    }
    fixes
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/imports/tests.rs"]
mod tests;
