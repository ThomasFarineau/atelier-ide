//! Tooltips of JavaScript code: what a name is (its declaration, type and JSDoc) and what a
//! member of a built-in or local type does.

use super::{
    analysis::{Analysis, Decl, DeclKind, Ty},
    builtins,
    complete::MAX_TEXT,
};
use crate::languages::Hover;

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn hover(text: &str, offset: usize) -> Option<Hover> {
    if text.len() > MAX_TEXT || offset > text.len() {
        return None;
    }
    let analysis = Analysis::new(text)?;
    let leaf = analysis.root().descendant_for_byte_range(offset, offset)?;
    // On the edge of a word, the pointer may be just past its last character.
    let leaf = match leaf.kind() {
        "identifier"
        | "property_identifier"
        | "shorthand_property_identifier"
        | "this"
        | "type_identifier"
        | "predefined_type" => leaf,
        _ => analysis
            .root()
            .descendant_for_byte_range(offset.saturating_sub(1), offset.saturating_sub(1))?,
    };
    // `string` is a token inside the node of the predefined type.
    let leaf = match leaf.parent() {
        Some(parent) if parent.kind() == "predefined_type" => parent,
        _ => leaf,
    };
    let range = leaf.byte_range();
    let name = &text[range.clone()];
    match leaf.kind() {
        "identifier" | "shorthand_property_identifier" => {
            if let Some(decl) = analysis.resolve(name, offset) {
                let (title, body) = describe_decl(&analysis, decl);
                return Some(Hover { range, title, body });
            }
            let (signature, doc) = builtins::global_doc(name)?;
            Some(Hover {
                range,
                title: format!("{name}: {signature}"),
                body: doc.to_string(),
            })
        }
        "type_identifier" | "predefined_type" => {
            if leaf.kind() == "type_identifier"
                && let Some(decl) = analysis.resolve(name, offset)
            {
                let (title, body) = describe_decl(&analysis, decl);
                return Some(Hover { range, title, body });
            }
            let doc = crate::languages::typescript::builtins::type_doc(name)?;
            Some(Hover {
                range,
                title: name.to_string(),
                body: doc.to_string(),
            })
        }
        "this" => {
            let ty = analysis.ty_of(leaf, offset);
            Some(Hover {
                range,
                title: format!("this: {}", analysis.show(&ty)),
                body: String::new(),
            })
        }
        "property_identifier" => {
            let parent = leaf.parent()?;
            if parent.kind() != "member_expression" {
                // The name of a method or a key being declared.
                return declared_member(&analysis, leaf, range);
            }
            let object = parent.child_by_field_name("object")?;
            let ty = analysis.ty_of(object, offset);
            let prop = analysis
                .props_of(&ty, offset)
                .into_iter()
                .find(|p| p.name == name)?;
            let title = if prop.method {
                format!("(method) {}{}", prop.name, short_signature(&prop.detail))
            } else {
                format!("(property) {}: {}", prop.name, prop.detail)
            };
            Some(Hover {
                range,
                title,
                body: prop.doc,
            })
        }
        _ => None,
    }
}

/// `(a, b) => any` shown after a method name as `(a, b)`.
fn short_signature(detail: &str) -> String {
    match detail.split_once(" => ") {
        Some((params, _)) => params.to_string(),
        None => detail.to_string(),
    }
}

fn declared_member(
    analysis: &Analysis,
    leaf: tree_sitter::Node,
    range: std::ops::Range<usize>,
) -> Option<Hover> {
    let parent = leaf.parent()?;
    if !matches!(
        parent.kind(),
        "method_definition"
            | "field_definition"
            | "pair"
            | "public_field_definition"
            | "property_signature"
            | "method_signature"
            | "abstract_method_signature"
            | "enum_assignment"
    ) {
        return None;
    }
    let doc = analysis
        .jsdoc_before(&parent.byte_range())
        .unwrap_or_default();
    let name = &analysis.text[range.clone()];
    let title = match parent.kind() {
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            format!(
                "(method) {name}{}",
                short_signature(&analysis.signature(&parent))
            )
        }
        "public_field_definition" | "property_signature" => {
            match parent.child_by_field_name("type") {
                Some(annotation) => format!(
                    "(property) {name}: {}",
                    analysis.annotation_text(annotation)
                ),
                None => format!("(property) {name}"),
            }
        }
        _ => format!("(property) {name}"),
    };
    Some(Hover {
        range,
        title,
        body: doc,
    })
}

fn describe_decl(analysis: &Analysis, decl: &Decl) -> (String, String) {
    let ty = analysis.decl_ty(decl, 0);
    let doc = analysis.jsdoc_before(&decl.node).unwrap_or_default();
    let title = match decl.kind {
        DeclKind::Function => {
            let header = analysis
                .node_at_range(&decl.node)
                .map(|n| {
                    let end = n
                        .child_by_field_name("body")
                        .map_or(n.end_byte(), |b| b.start_byte());
                    collapse(&analysis.text[n.start_byte()..end])
                })
                .unwrap_or_else(|| decl.name.clone());
            header
        }
        DeclKind::Class => analysis
            .node_at_range(&decl.node)
            .map(|n| {
                let end = n
                    .child_by_field_name("body")
                    .map_or(n.end_byte(), |b| b.start_byte());
                collapse(&analysis.text[n.start_byte()..end])
            })
            .unwrap_or_else(|| format!("class {}", decl.name)),
        DeclKind::Interface | DeclKind::Enum | DeclKind::Namespace => analysis
            .node_at_range(&decl.node)
            .map(|n| {
                let end = n
                    .child_by_field_name("body")
                    .map_or(n.end_byte(), |b| b.start_byte());
                collapse(&analysis.text[n.start_byte()..end])
            })
            .unwrap_or_else(|| decl.name.clone()),
        DeclKind::TypeAlias => analysis
            .node_at_range(&decl.node)
            .map(|n| {
                let text = collapse(&analysis.text[n.byte_range()]);
                let text = text.trim_end_matches(';').to_string();
                if text.chars().count() > 160 {
                    format!("{} …", text.chars().take(160).collect::<String>())
                } else {
                    text
                }
            })
            .unwrap_or_else(|| decl.name.clone()),
        DeclKind::Param => format!("(parameter) {}: {}", decl.name, analysis.show(&ty)),
        DeclKind::Catch => format!("(parameter) {}: any", decl.name),
        DeclKind::Import => format!("(import) {}", decl.name),
        DeclKind::Const | DeclKind::Let | DeclKind::Var => {
            let keyword = match decl.kind {
                DeclKind::Const => "const",
                DeclKind::Let => "let",
                _ => "var",
            };
            format!("{keyword} {}: {}", decl.name, analysis.show(&ty))
        }
    };
    let body = match (&decl.import, doc.is_empty()) {
        (Some((module, _)), _) => format!("depuis « {module} »"),
        (None, false) => doc,
        (None, true) => String::new(),
    };
    let _ = Ty::Unknown;
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(source: &str) -> (String, usize) {
        let offset = source.find('|').unwrap();
        (source.replacen('|', "", 1), offset)
    }

    #[test]
    fn names_show_their_declaration_type_and_documentation() {
        let (text, offset) =
            at("/** Adds two numbers. */\nfunction add(a, b) { return a + b; }\nadd|(1, 2);\n");
        let info = hover(&text, offset - 1).unwrap();
        assert_eq!(info.title, "function add(a, b)");
        assert_eq!(info.body, "Adds two numbers.");

        let (text, offset) = at("const total = 5;\nconsole.log(tot|al);\n");
        let info = hover(&text, offset).unwrap();
        assert_eq!(info.title, "const total: number");
        assert_eq!(&text[info.range.clone()], "total");

        let (text, offset) = at("function f(items) { it|ems; }\n");
        assert_eq!(
            hover(&text, offset).unwrap().title,
            "(parameter) items: any"
        );

        let (text, offset) = at("class Dog extends Animal { bark() {} }\nnew Do|g();\n");
        assert_eq!(
            hover(&text, offset).unwrap().title,
            "class Dog extends Animal"
        );
    }

    #[test]
    fn built_ins_and_members_are_documented() {
        let (text, offset) = at("setTime|out(() => {}, 10);\n");
        let info = hover(&text, offset).unwrap();
        assert!(info.title.starts_with("setTimeout: "), "{}", info.title);
        assert!(info.body.contains("délai"));

        let (text, offset) = at("const s = 'x';\ns.toUpper|Case();\n");
        let info = hover(&text, offset).unwrap();
        assert_eq!(info.title, "(method) toUpperCase(): string");
        assert!(!info.body.is_empty());

        let (text, offset) = at("Math.fl|oor(1.5);\n");
        assert!(
            hover(&text, offset)
                .unwrap()
                .title
                .starts_with("(method) floor")
        );
        let (text, offset) = at("let x = 1; // no|te\n");
        assert!(hover(&text, offset).is_none());
    }
}
