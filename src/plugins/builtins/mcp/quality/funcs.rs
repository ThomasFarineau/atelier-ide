//! Functions of a parsed file, their namespace and their cyclomatic complexity.

use super::lang::Lang;
use std::path::Path;
use tree_sitter::{Node, Tree};

pub struct Func<'t> {
    pub name: String,
    pub namespace: String,
    pub node: Node<'t>,
    pub start_line: usize,
    pub end_line: usize,
}

fn text<'a>(n: Node, src: &'a str) -> &'a str {
    &src[n.byte_range()]
}

fn push<'t>(out: &mut Vec<Func<'t>>, name: &str, namespace: String, node: Node<'t>) {
    out.push(Func {
        name: name.to_string(),
        namespace,
        node,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    });
}

fn children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

struct Ctx<'a> {
    src: &'a str,
    module: &'a str,
}

type Walker<'t> = fn(Node<'t>, Option<&str>, &Ctx, &mut Vec<Func<'t>>);

impl Ctx<'_> {
    fn text(&self, n: Node) -> &str {
        text(n, self.src)
    }

    fn script_namespace(&self, class: Option<&str>) -> String {
        class.map_or(self.module.to_string(), |c| format!("{}.{c}", self.module))
    }

    /// Record `body` under the `name` field of `named`.
    fn push_named<'t>(
        &self,
        named: Node,
        body: Node<'t>,
        namespace: String,
        out: &mut Vec<Func<'t>>,
    ) {
        if let Some(n) = named.child_by_field_name("name") {
            push(out, self.text(n), namespace, body);
        }
    }

    fn walk_children<'t>(
        &self,
        node: Node<'t>,
        owner: Option<&str>,
        out: &mut Vec<Func<'t>>,
        walk: Walker<'t>,
    ) {
        for child in children(node) {
            walk(child, owner, self, out);
        }
    }
}

pub fn functions<'t>(tree: &'t Tree, src: &str, lang: Lang, module: &str) -> Vec<Func<'t>> {
    let mut out = Vec::new();
    let ctx = Ctx { src, module };
    let walk: Walker = if lang.is_rust() {
        walk_rust
    } else {
        walk_script
    };
    walk(tree.root_node(), None, &ctx, &mut out);
    out
}

fn walk_script<'t>(node: Node<'t>, class: Option<&str>, ctx: &Ctx, out: &mut Vec<Func<'t>>) {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" | "method_definition" => {
            ctx.push_named(node, node, ctx.script_namespace(class), out)
        }
        "class_declaration" | "abstract_class_declaration" | "class" => {
            let name = node.child_by_field_name("name").map(|n| ctx.text(n));
            ctx.walk_children(node, name.or(class), out, walk_script)
        }
        "variable_declarator" => match node.child_by_field_name("value") {
            Some(value)
                if matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "function"
                ) =>
            {
                ctx.push_named(node, value, ctx.script_namespace(class), out)
            }
            _ => ctx.walk_children(node, class, out, walk_script),
        },
        _ => ctx.walk_children(node, class, out, walk_script),
    }
}

fn marked_test(node: Node, src: &str) -> bool {
    let mut prev = node.prev_sibling();
    while let Some(p) = prev.filter(|p| p.kind() == "attribute_item") {
        let t = text(p, src);
        if t.contains("cfg(test)") || t.trim_end_matches(']').ends_with("test") {
            return true;
        }
        prev = p.prev_sibling();
    }
    false
}

fn walk_rust<'t>(node: Node<'t>, owner: Option<&str>, ctx: &Ctx, out: &mut Vec<Func<'t>>) {
    match node.kind() {
        "function_item" => rust_function(node, owner, ctx, out),
        "impl_item" | "trait_item" => {
            let field = if node.kind() == "impl_item" {
                "type"
            } else {
                "name"
            };
            let owner = node
                .child_by_field_name(field)
                .map(|n| ctx.text(n).split('<').next().unwrap_or("").trim());
            ctx.walk_children(node, owner, out, walk_rust)
        }
        "mod_item" => rust_mod(node, ctx, out),
        _ => ctx.walk_children(node, owner, out, walk_rust),
    }
}

fn rust_function<'t>(node: Node<'t>, owner: Option<&str>, ctx: &Ctx, out: &mut Vec<Func<'t>>) {
    if !marked_test(node, ctx.src) {
        let ns = owner.map_or(ctx.module.to_string(), |o| format!("{}::{o}", ctx.module));
        ctx.push_named(node, node, ns, out);
    }
}

fn rust_mod<'t>(node: Node<'t>, ctx: &Ctx, out: &mut Vec<Func<'t>>) {
    let name = node.child_by_field_name("name").map_or("", |n| ctx.text(n));
    if marked_test(node, ctx.src) || name == "tests" {
        return;
    }
    let module = format!("{}::{name}", ctx.module);
    let inner = Ctx {
        src: ctx.src,
        module: &module,
    };
    inner.walk_children(node, None, out, walk_rust);
}

/// Dotted (or `::`) module path used as the namespace of a file's functions.
pub fn module_name(root: &Path, file: &Path, lang: Lang) -> String {
    if lang.is_rust() {
        return rust_module(root, file);
    }
    let mut parts = path_parts(file.strip_prefix(root).unwrap_or(file));
    if parts.len() > 1 && parts[0] == "src" {
        parts.remove(0);
    }
    parts.join(".")
}

fn path_parts(rel: &Path) -> Vec<String> {
    rel.with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect()
}

fn crate_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with("name"))?;
    Some(line.split('"').nth(1)?.replace('-', "_"))
}

/// Module segments of a file inside its crate: `src/a/mod.rs` is `a`, `src/lib.rs` is nothing.
fn rust_segments(rel: &Path) -> Vec<String> {
    let mut parts = path_parts(rel);
    if parts.first().is_some_and(|p| p == "src") {
        parts.remove(0);
    }
    if parts.len() == 1 && (parts[0] == "lib" || parts[0] == "main") {
        parts.clear();
    }
    if parts.last().is_some_and(|p| p == "mod") {
        parts.pop();
    }
    parts
}

fn rust_module(root: &Path, file: &Path) -> String {
    let mut ancestors = file.ancestors().take_while(|d| d.starts_with(root));
    let Some(crate_dir) = ancestors.find(|d| d.join("Cargo.toml").is_file()) else {
        return file
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().to_string());
    };
    let krate = crate_name(&crate_dir.join("Cargo.toml"))
        .or_else(|| {
            crate_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_default();
    let segments = rust_segments(file.strip_prefix(crate_dir).unwrap_or(file));
    std::iter::once(krate)
        .chain(segments)
        .collect::<Vec<_>>()
        .join("::")
}

fn is_decision(n: Node) -> bool {
    match n.kind() {
        "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
        | "do_statement" | "catch_clause" | "ternary_expression" | "switch_case"
        | "optional_chain" | "if_expression" | "while_expression" | "for_expression"
        | "loop_expression" | "match_arm" | "try_expression" => true,
        "binary_expression" => n
            .child_by_field_name("operator")
            .is_some_and(|o| matches!(o.kind(), "&&" | "||" | "??")),
        _ => false,
    }
}

fn decisions(n: Node) -> usize {
    usize::from(is_decision(n)) + children(n).into_iter().map(decisions).sum::<usize>()
}

/// Cyclomatic complexity: one plus the decision points, nested closures included.
pub fn complexity(node: Node) -> usize {
    1 + decisions(node)
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/funcs/tests.rs"]
mod tests;
