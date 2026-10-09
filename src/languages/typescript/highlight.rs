//! Syntax colors of TypeScript from its syntax tree and the highlight queries of the grammars,
//! which know the whole language (types, generics, decorators, JSX) where the JavaScript
//! syntax of the editor does not.

use eframe::egui::Color32;
use std::{ops::Range, sync::OnceLock};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Largest document colored this way.
const MAX_TEXT: usize = 600_000;

/// The colors of the editor scheme that the capture names map to.
#[derive(Clone, Copy)]
pub struct Palette {
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub function: Color32,
    pub constant: Color32,
    pub annotation: Color32,
    pub tag: Color32,
    pub escape: Color32,
    pub text: Color32,
}

struct Grammar {
    language: Language,
    queries: Vec<Query>,
}

fn grammar(tsx: bool) -> &'static Grammar {
    static TS: OnceLock<Grammar> = OnceLock::new();
    static TSX: OnceLock<Grammar> = OnceLock::new();
    let cell = if tsx { &TSX } else { &TS };
    cell.get_or_init(|| {
        let language: Language = if tsx {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        };
        // The queries of the JavaScript grammar first, then the ones of TypeScript, which take
        // precedence. A query that does not fit the grammar is left out.
        let mut sources = vec![tree_sitter_javascript::HIGHLIGHT_QUERY];
        if tsx {
            sources.push(tree_sitter_javascript::JSX_HIGHLIGHT_QUERY);
        }
        sources.push(tree_sitter_typescript::HIGHLIGHTS_QUERY);
        let queries = sources
            .into_iter()
            .filter_map(|source| Query::new(&language, source).ok())
            .collect();
        Grammar { language, queries }
    })
}

/// Which color a capture name stands for: 0 none, 1 keyword, 2 string, 3 number, 4 comment,
/// 5 function, 6 constant, 7 annotation, 8 tag, 9 escape.
fn code_of(name: &str) -> u8 {
    match name {
        n if n.starts_with("keyword") || n == "type.builtin" || n == "variable.builtin" || n == "constant.builtin" => 1,
        n if n.starts_with("string") || n.starts_with("template") => 2,
        "number" | "float" => 3,
        n if n.starts_with("comment") => 4,
        n if n.starts_with("function") => 5,
        "constant" => 6,
        "attribute" | "decorator" => 7,
        "tag" => 8,
        "escape" | "string.escape" => 9,
        _ => 0,
    }
}

/// Parses with the grammar that fits: TSX only when plain TypeScript does not parse.
fn parse(text: &str) -> Option<(tree_sitter::Tree, bool)> {
    let mut best: Option<(tree_sitter::Tree, bool)> = None;
    for tsx in [false, true] {
        let mut parser = Parser::new();
        parser.set_language(&grammar(tsx).language).ok()?;
        let tree = parser.parse(text, None)?;
        let clean = !tree.root_node().has_error();
        if clean {
            return Some((tree, tsx));
        }
        if best.is_none() {
            best = Some((tree, tsx));
        }
    }
    best
}

/// The colored ranges of each line of the text.
pub fn colors(text: &str, palette: &Palette) -> Option<Vec<Vec<(Range<usize>, Color32)>>> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let (tree, tsx) = parse(text)?;
    let grammar = grammar(tsx);
    if grammar.queries.is_empty() {
        return None;
    }
    // (priority, range, code): a later pattern of a later query wins.
    let mut marks: Vec<(usize, Range<usize>, u8)> = Vec::new();
    let mut offset = 0usize;
    for query in &grammar.queries {
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), text.as_bytes());
        while let Some(found) = matches.next() {
            for capture in found.captures() {
                let code = code_of(names[capture.index as usize]);
                if code != 0 {
                    marks.push((offset + found.pattern_index, capture.node.byte_range(), code));
                }
            }
        }
        offset += query.pattern_count();
    }
    marks.sort_by_key(|(priority, _, _)| *priority);
    let mut paint = vec![0u8; text.len()];
    for (_, range, code) in marks {
        let end = range.end.min(paint.len());
        for slot in &mut paint[range.start.min(end)..end] {
            *slot = code;
        }
    }
    let color = |code: u8| match code {
        1 => palette.keyword,
        2 => palette.string,
        3 => palette.number,
        4 => palette.comment,
        5 => palette.function,
        6 => palette.constant,
        7 => palette.annotation,
        8 => palette.tag,
        9 => palette.escape,
        _ => palette.text,
    };
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        let mut spans: Vec<(Range<usize>, Color32)> = Vec::new();
        let bytes = &paint[start..start + line.len()];
        let mut i = 0;
        while i < bytes.len() {
            let code = bytes[i];
            let from = i;
            while i < bytes.len() && bytes[i] == code {
                i += 1;
            }
            if code != 0 && line.is_char_boundary(from) && line.is_char_boundary(i) {
                spans.push((from..i, color(code)));
            }
        }
        lines.push(spans);
        start += line.len() + 1;
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let c = |n: u8| Color32::from_rgb(n, n, n);
        Palette { keyword: c(1), string: c(2), number: c(3), comment: c(4), function: c(5), constant: c(6), annotation: c(7), tag: c(8), escape: c(9), text: c(0) }
    }

    fn color_of(lines: &[Vec<(Range<usize>, Color32)>], line: usize, text: &str, word: &str) -> Option<u8> {
        let source = text.lines().nth(line).unwrap();
        let at = source.find(word)?;
        lines[line].iter().find(|(r, _)| r.start <= at && at < r.end).map(|(_, c)| c.r())
    }

    #[test]
    fn the_whole_typescript_syntax_is_colored() {
        let text = "interface User {\n  name: string;\n  count?: number;\n}\n// note\ntype Id = string | number;\nenum Color { Red = 1 }\nexport class Box<T> implements User {\n  private readonly items: T[] = [];\n  constructor(public label: string) {}\n  add(item: T): void { this.items.push(item); }\n}\nconst msg: string = `hi ${1}`;\nlet n = 42 as const;\n@Injectable()\nclass Service {}\n";
        let lines = colors(text, &palette()).expect("the queries compile");
        assert_eq!(lines.len(), text.split('\n').count());
        assert_eq!(color_of(&lines, 0, text, "interface"), Some(1), "interface");
        assert_eq!(color_of(&lines, 1, text, "string"), Some(1), "a primitive type");
        assert_eq!(color_of(&lines, 2, text, "number"), Some(1));
        assert_eq!(color_of(&lines, 4, text, "note"), Some(4), "a comment");
        assert_eq!(color_of(&lines, 5, text, "type"), Some(1), "type alias");
        assert_eq!(color_of(&lines, 6, text, "enum"), Some(1));
        assert_eq!(color_of(&lines, 7, text, "implements"), Some(1));
        assert_eq!(color_of(&lines, 8, text, "private"), Some(1), "a modifier");
        assert_eq!(color_of(&lines, 8, text, "readonly"), Some(1));
        assert_eq!(color_of(&lines, 12, text, "hi"), Some(2), "a template string");
        assert_eq!(color_of(&lines, 13, text, "42"), Some(3));
        assert_eq!(color_of(&lines, 13, text, "const"), Some(1));
    }

    #[test]
    fn tsx_is_colored_when_typescript_alone_does_not_parse() {
        let text = "const App = () => <div className=\"a\">{1}</div>;\n";
        let lines = colors(text, &palette()).expect("the tsx grammar is used");
        assert_eq!(color_of(&lines, 0, text, "\"a\""), Some(2));
    }
}
