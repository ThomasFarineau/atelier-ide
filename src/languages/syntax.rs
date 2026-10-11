//! Syntax errors from a tree-sitter tree, for the languages whose analysis is a grammar
//! (Rust, Java): a token the parser expected and did not find, or code it could not read.

use super::{Diagnostic, Severity};
use tree_sitter::{Node, Tree};

const MAX_ERRORS: usize = 50;

/// The errors of a tree. `names` are the kinds of the grammar that are names, for the message
/// "identifier expected".
pub fn errors(tree: &Tree, src: &str, names: &[&str]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    walk(tree.root_node(), src, names, &mut out);
    out
}

fn walk(node: Node, src: &str, names: &[&str], out: &mut Vec<Diagnostic>) {
    if out.len() >= MAX_ERRORS || !node.has_error() {
        return;
    }
    if node.is_missing() {
        let kind = node.kind();
        let message = if names.contains(&kind) {
            tr!("syntax.identifier_expected").to_string()
        } else {
            trf!("syntax.expected", kind = kind)
        };
        // A missing token has no width: show the character before it.
        let at = node.start_byte();
        let start = src[..at].char_indices().next_back().map_or(at, |(i, _)| i);
        out.push(Diagnostic {
            range: start..at.max(start + 1).min(src.len()),
            severity: Severity::Error,
            message,
            source: tr!("syntax").into(),
        });
        return;
    }
    if node.is_error() {
        let mut range = node.byte_range();
        // The error node spans everything the parser gave up on: point at its first token.
        if let Some(first) = node.child(0)
            && first.byte_range().len() < range.len()
        {
            range = first.byte_range();
        }
        if range.len() > 40 {
            let mut end = range.start + 40;
            while !src.is_char_boundary(end) {
                end -= 1;
            }
            range.end = end;
        }
        let shown = src[range.clone()].trim();
        let message = if shown.is_empty() || shown.contains('\n') {
            tr!("syntax.invalid").to_string()
        } else {
            trf!("syntax.unexpected", shown = shown)
        };
        out.push(Diagnostic {
            range,
            severity: Severity::Error,
            message,
            source: tr!("syntax").into(),
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, names, out);
    }
}
