//! The languages the quality tools understand, and how to parse them.

use std::path::Path;
use tree_sitter::{Language, Parser, Tree};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ts,
    Tsx,
    Js,
    Rust,
}

const EXTENSIONS: &[(&[&str], Lang)] = &[
    (&["ts", "mts", "cts"], Lang::Ts),
    (&["tsx"], Lang::Tsx),
    (&["js", "jsx", "mjs", "cjs"], Lang::Js),
    (&["rs"], Lang::Rust),
];

impl Lang {
    pub fn from_path(path: &Path) -> Option<Lang> {
        let name = path.file_name()?.to_str()?;
        if name.ends_with(".d.ts") {
            return None;
        }
        let ext = path.extension()?.to_str()?;
        EXTENSIONS
            .iter()
            .find(|(exts, _)| exts.contains(&ext))
            .map(|(_, lang)| *lang)
    }

    fn language(self) -> Language {
        match self {
            Lang::Ts => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Js => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    pub fn is_rust(self) -> bool {
        self == Lang::Rust
    }

    /// Name shown in reports.
    pub fn name(self) -> &'static str {
        if self == Lang::Js {
            "javascript"
        } else if self.is_rust() {
            "rust"
        } else {
            "typescript"
        }
    }
}

pub fn parse(lang: Lang, src: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&lang.language()).ok()?;
    parser.parse(src, None)
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/lang/tests.rs"]
mod tests;
