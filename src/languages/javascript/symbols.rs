//! The structure of a JavaScript file: its functions, classes, methods and constants.

use super::analysis::Analysis;
use crate::languages::{ItemKind, Symbol};
use tree_sitter::Node;

const MAX_SYMBOLS: usize = 2000;

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn params(node: Node, src: &str) -> String {
    node.child_by_field_name("parameters")
        .map(|p| {
            text(p, src)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .or_else(|| {
            node.child_by_field_name("parameter")
                .map(|p| format!("({})", text(p, src)))
        })
        .unwrap_or_else(|| "()".into())
}

fn is_function(node: Node) -> bool {
    matches!(
        node.kind(),
        "function_expression"
            | "arrow_function"
            | "generator_function"
            | "function_declaration"
            | "generator_function_declaration"
    )
}

pub fn symbols(source: &str) -> Vec<Symbol> {
    let Some(analysis) = Analysis::new(source) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect(analysis.root(), source, 0, &mut out);
    out
}

fn collect(parent: Node, src: &str, depth: usize, out: &mut Vec<Symbol>) {
    let mut cursor = parent.walk();
    for node in parent.named_children(&mut cursor) {
        if out.len() >= MAX_SYMBOLS {
            return;
        }
        match node.kind() {
            "export_statement" | "ambient_declaration" | "expression_statement" => {
                collect(node, src, depth, out)
            }
            "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "internal_module" => {
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                let (kind, detail) = match node.kind() {
                    "interface_declaration" => (ItemKind::Interface, {
                        let mut inner = node.walk();
                        node.children(&mut inner)
                            .find(|c| c.kind() == "extends_type_clause")
                            .map(|c| {
                                text(c, src)
                                    .split_whitespace()
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default()
                    }),
                    "type_alias_declaration" => (ItemKind::Interface, "type".to_string()),
                    "enum_declaration" => (ItemKind::Enum, String::new()),
                    _ => (ItemKind::Module, "namespace".to_string()),
                };
                out.push(Symbol {
                    name: text(name, src).to_string(),
                    kind,
                    detail,
                    range: name.byte_range(),
                    depth,
                });
                match node.kind() {
                    "interface_declaration" => {
                        if let Some(body) = node.child_by_field_name("body") {
                            signature_members(body, src, depth + 1, out);
                        }
                    }
                    "type_alias_declaration" => {
                        if let Some(value) = node
                            .child_by_field_name("value")
                            .filter(|v| v.kind() == "object_type")
                        {
                            signature_members(value, src, depth + 1, out);
                        }
                    }
                    "enum_declaration" => {
                        if let Some(body) = node.child_by_field_name("body") {
                            let mut inner = body.walk();
                            for member in body.named_children(&mut inner) {
                                let name = if member.kind() == "enum_assignment" {
                                    member.child_by_field_name("name")
                                } else {
                                    Some(member)
                                };
                                if let Some(name) =
                                    name.filter(|n| n.kind() == "property_identifier")
                                {
                                    out.push(Symbol {
                                        name: text(name, src).to_string(),
                                        kind: ItemKind::Constant,
                                        detail: String::new(),
                                        range: name.byte_range(),
                                        depth: depth + 1,
                                    });
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(body) = node.child_by_field_name("body") {
                            collect(body, src, depth + 1, out);
                        }
                    }
                }
            }
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(Symbol {
                        name: text(name, src).to_string(),
                        kind: ItemKind::Function,
                        detail: params(node, src),
                        range: name.byte_range(),
                        depth,
                    });
                    // Functions declared inside are part of the structure too.
                    if depth < 2
                        && let Some(body) = node.child_by_field_name("body")
                    {
                        collect(body, src, depth + 1, out);
                    }
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                let extends = {
                    let mut inner = node.walk();
                    node.children(&mut inner)
                        .find(|c| c.kind() == "class_heritage")
                        .map(|h| text(h, src).trim().to_string())
                        .unwrap_or_default()
                };
                out.push(Symbol {
                    name: text(name, src).to_string(),
                    kind: ItemKind::Class,
                    detail: extends,
                    range: name.byte_range(),
                    depth,
                });
                if let Some(body) = node.child_by_field_name("body") {
                    class_members(body, src, depth + 1, out);
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                let mut inner = node.walk();
                for declarator in node
                    .named_children(&mut inner)
                    .filter(|d| d.kind() == "variable_declarator")
                {
                    let (Some(name), value) = (
                        declarator.child_by_field_name("name"),
                        declarator.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    if name.kind() != "identifier" {
                        continue;
                    }
                    let (kind, detail) = match value {
                        Some(v) if is_function(v) => (ItemKind::Function, params(v, src)),
                        Some(v) if v.kind() == "class" => (ItemKind::Class, String::new()),
                        _ if node.child(0).is_some_and(|k| text(k, src) == "const") => {
                            (ItemKind::Constant, String::new())
                        }
                        _ => (ItemKind::Variable, String::new()),
                    };
                    // Local variables of the module only: not those inside functions.
                    if depth == 0 || kind == ItemKind::Function {
                        out.push(Symbol {
                            name: text(name, src).to_string(),
                            kind,
                            detail,
                            range: name.byte_range(),
                            depth,
                        });
                    }
                    if let Some(value) = value
                        && value.kind() == "class"
                        && let Some(body) = value.child_by_field_name("body")
                    {
                        class_members(body, src, depth + 1, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The properties and methods of an interface or an object type.
fn signature_members(body: Node, src: &str, depth: usize, out: &mut Vec<Symbol>) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        let Some(name) = member.child_by_field_name("name") else {
            continue;
        };
        let (kind, detail) = match member.kind() {
            "method_signature" => (ItemKind::Method, params(member, src)),
            "property_signature" => (
                ItemKind::Property,
                member
                    .child_by_field_name("type")
                    .map(|t| text(t, src).trim_start_matches(':').trim().to_string())
                    .unwrap_or_default(),
            ),
            _ => continue,
        };
        out.push(Symbol {
            name: text(name, src).to_string(),
            kind,
            detail,
            range: name.byte_range(),
            depth,
        });
    }
}

fn class_members(body: Node, src: &str, depth: usize, out: &mut Vec<Symbol>) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        let (name, kind, detail) = match member.kind() {
            "method_definition" => {
                let Some(name) = member.child_by_field_name("name") else {
                    continue;
                };
                let kind = if text(name, src) == "constructor" {
                    ItemKind::Method
                } else {
                    ItemKind::Method
                };
                (name, kind, params(member, src))
            }
            "field_definition" | "public_field_definition" => {
                let Some(name) = member
                    .child_by_field_name("property")
                    .or_else(|| member.child_by_field_name("name"))
                else {
                    continue;
                };
                let is_fn = member.child_by_field_name("value").is_some_and(is_function);
                let detail = member
                    .child_by_field_name("type")
                    .map(|t| text(t, src).trim_start_matches(':').trim().to_string())
                    .unwrap_or_default();
                (
                    name,
                    if is_fn {
                        ItemKind::Method
                    } else {
                        ItemKind::Property
                    },
                    detail,
                )
            }
            "abstract_method_signature" => {
                let Some(name) = member.child_by_field_name("name") else {
                    continue;
                };
                (name, ItemKind::Method, params(member, src))
            }
            _ => continue,
        };
        out.push(Symbol {
            name: text(name, src).to_string(),
            kind,
            detail,
            range: name.byte_range(),
            depth,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_structure_lists_functions_classes_members_and_constants() {
        let source = "const LIMIT = 3;\nlet counter = 0;\nexport function add(a, b) {\n  function inner() {}\n  return a + b;\n}\nconst twice = (x) => x * 2;\nclass Dog extends Animal {\n  name = 'rex';\n  constructor(n) {}\n  bark() {}\n  static create() {}\n}\n";
        let all = symbols(source);
        let summary: Vec<(String, ItemKind, usize)> = all
            .iter()
            .map(|s| (s.name.clone(), s.kind, s.depth))
            .collect();
        assert_eq!(
            summary,
            [
                ("LIMIT".to_string(), ItemKind::Constant, 0),
                ("counter".to_string(), ItemKind::Variable, 0),
                ("add".to_string(), ItemKind::Function, 0),
                ("inner".to_string(), ItemKind::Function, 1),
                ("twice".to_string(), ItemKind::Function, 0),
                ("Dog".to_string(), ItemKind::Class, 0),
                ("name".to_string(), ItemKind::Property, 1),
                ("constructor".to_string(), ItemKind::Method, 1),
                ("bark".to_string(), ItemKind::Method, 1),
                ("create".to_string(), ItemKind::Method, 1),
            ]
        );
        assert_eq!(all[2].detail, "(a, b)");
        assert_eq!(all[5].detail, "extends Animal");
        assert_eq!(&source[all[2].range.clone()], "add");
    }
}
