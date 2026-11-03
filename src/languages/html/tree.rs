//! The elements of an HTML document, from its tags: which closing tag ends which element, the
//! closing tags that HTML lets you leave out (`<li>`, `<p>`, `<td>`…), the void elements, and
//! what is left unclosed or closed without having been opened.

use super::{
    data,
    lexer::{Scan, Tag},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// Its opening tag, in [`Scan::tags`].
    pub open: usize,
    /// Its closing tag, when it has one.
    pub close: Option<usize>,
    pub parent: Option<usize>,
    pub depth: usize,
    /// Where its content ends: its closing tag, or where it was closed without one.
    pub end: usize,
}

#[derive(Debug, Default)]
pub struct Tree {
    pub elements: Vec<Element>,
    /// The element of each opening tag (None for the closing tags).
    pub of_tag: Vec<Option<usize>>,
    /// The element each closing tag ends.
    pub closed_by: Vec<Option<usize>>,
    /// Closing tags that end nothing.
    pub stray: Vec<usize>,
    /// Elements whose closing tag is missing although HTML requires it.
    pub unclosed: Vec<usize>,
}

impl Tree {
    pub fn name<'a>(&self, scan: &'a Scan, element: usize) -> &'a str {
        &scan.tags[self.elements[element].open].name
    }

    pub fn open_tag<'a>(&self, scan: &'a Scan, element: usize) -> &'a Tag {
        &scan.tags[self.elements[element].open]
    }
}

/// The elements of a scanned document.
pub fn build(scan: &Scan) -> Tree {
    let mut tree = Tree {
        of_tag: vec![None; scan.tags.len()],
        closed_by: vec![None; scan.tags.len()],
        ..Tree::default()
    };
    let mut stack: Vec<usize> = Vec::new();
    for (index, tag) in scan.tags.iter().enumerate() {
        if tag.closing {
            let found = stack
                .iter()
                .rposition(|&e| scan.tags[tree.elements[e].open].name == tag.name);
            match found {
                Some(position) => {
                    // What is still open above it ends here.
                    for &inner in &stack[position + 1..] {
                        tree.elements[inner].end = tag.range.start;
                        if !data::optional_end(&scan.tags[tree.elements[inner].open].name) {
                            tree.unclosed.push(inner);
                        }
                    }
                    let element = stack[position];
                    tree.elements[element].close = Some(index);
                    tree.elements[element].end = tag.range.start;
                    tree.closed_by[index] = Some(element);
                    stack.truncate(position);
                }
                // `</br>` and `</p>` are read as tags by browsers: no error for those.
                None if data::is_void(&tag.name) || tag.name == "p" => {}
                None => tree.stray.push(index),
            }
            continue;
        }
        // An element whose closing tag may be left out ends where the next one starts.
        while let Some(&top) = stack.last() {
            let name = &scan.tags[tree.elements[top].open].name;
            if data::optional_end(name) && data::closes(name, &tag.name) {
                tree.elements[top].end = tag.range.start;
                stack.pop();
            } else {
                break;
            }
        }
        let element = tree.elements.len();
        tree.elements.push(Element {
            open: index,
            close: None,
            parent: stack.last().copied(),
            depth: stack.len(),
            end: tag.range.end,
        });
        tree.of_tag[index] = Some(element);
        if !data::is_void(&tag.name) && !tag.self_closing && tag.complete {
            stack.push(element);
        }
    }
    let end = scan.tokens.last().map_or(0, |t| t.range.end);
    for element in stack {
        tree.elements[element].end = end;
        if !data::optional_end(&scan.tags[tree.elements[element].open].name) {
            tree.unclosed.push(element);
        }
    }
    tree.unclosed.sort_unstable();
    tree
}
