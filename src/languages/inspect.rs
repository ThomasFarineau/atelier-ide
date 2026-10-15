//! Inspecting files that are not open: the syntax errors of the files of a project, read without
//! a language server, for Inspect Code and for the check before a commit.

use super::{Diagnostic, javascript, rust};
use std::path::Path;

/// Files bigger than this are not inspected: the check is a comfort, not worth a stall.
pub const MAX_SIZE: u64 = 1_500_000;

/// The language of a source file as `project::language` names it, when the inspection can read it.
pub fn language_of(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => "rs",
        "java" => "java",
        "js" | "jsx" | "mjs" | "cjs" => "js",
        "ts" | "tsx" | "mts" | "cts" => "ts",
        _ => return None,
    })
}

/// The syntax errors of a file.
pub fn syntax_errors(path: &Path, text: &str) -> Vec<Diagnostic> {
    match language_of(path) {
        Some("rs") => rust::analysis::syntax_errors(text),
        Some("java") => super::java::analysis::syntax_errors(text),
        Some(language) => {
            let _dialect = javascript::analysis::use_dialect(
                javascript::analysis::Dialect::of(Some(path), language),
            );
            javascript::analysis::syntax_errors(text)
        }
        None => Vec::new(),
    }
}

#[cfg(test)]
#[path = "../../test/languages/inspect/tests.rs"]
mod tests;
