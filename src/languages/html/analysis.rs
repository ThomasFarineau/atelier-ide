//! What the IDE knows of HTML and Pug by itself: the syntax errors, the structure, the
//! completion of elements, attributes, values and entities, the documentation of the elements,
//! the occurrences and the declaration of ids, classes, mixins and tag pairs.
//!
//! The code inside a page (`<script>`, `<style>`) is not looked at here: its own language
//! answers for it (see `embedded`).

use super::{
    data, embedded,
    lexer::{self, Kind, Scan},
    pug,
    tree::{self, Tree},
};
use crate::languages::{
    Completion, CompletionItem, Diagnostic, Hover, ItemKind, Location, Severity, Symbol,
};
use std::ops::Range;

/// Larger documents get no analysis of their own (only the colors and the server).
pub const MAX_TEXT: usize = 2_000_000;

fn diagnostic(range: Range<usize>, severity: Severity, message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity,
        message,
        source: tr!("html.source").into(),
    }
}

/// The description of an element, in the language of the interface.
pub fn description(element: &str) -> Option<&'static str> {
    crate::i18n::lookup(&format!("html.element.{element}"))
}

/// Whether `name` is in an `<svg>` or a `<math>`, whose elements are not the ones of HTML.
fn foreign(scan: &Scan, tree: &Tree, element: usize) -> bool {
    let mut current = Some(element);
    while let Some(e) = current {
        if matches!(tree.name(scan, e), "svg" | "math") {
            return true;
        }
        current = tree.elements[e].parent;
    }
    false
}

// ---- HTML ---------------------------------------------------------------------------------

pub fn syntax_errors(text: &str) -> Vec<Diagnostic> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = lexer::scan(text);
    let tree = tree::build(&scan);
    let mut out = Vec::new();
    for (range, what) in &scan.unclosed {
        let message = match what {
            lexer::Unclosed::Comment => tr!("html.unclosed_comment").to_string(),
            lexer::Unclosed::Tag => tr!("html.unclosed_tag").to_string(),
            lexer::Unclosed::Quote => tr!("html.unclosed_quote").to_string(),
        };
        out.push(diagnostic(range.clone(), Severity::Error, message));
    }
    for &tag in &tree.stray {
        let tag = &scan.tags[tag];
        out.push(diagnostic(
            tag.name_range.clone(),
            Severity::Error,
            trf!("html.stray_closing_tag", name = tag.name),
        ));
    }
    for &element in &tree.unclosed {
        let tag = tree.open_tag(&scan, element);
        out.push(diagnostic(
            tag.name_range.clone(),
            Severity::Warning,
            trf!("html.element_not_closed", name = tag.name),
        ));
    }
    let mut ids: Vec<&str> = Vec::new();
    for (index, element) in tree.elements.iter().enumerate() {
        let tag = &scan.tags[element.open];
        let known = data::is_element(&tag.name) || tag.name.contains('-');
        if !known && !foreign(&scan, &tree, index) {
            out.push(diagnostic(
                tag.name_range.clone(),
                Severity::Warning,
                trf!("html.unknown_element", name = tag.name),
            ));
        }
        for (i, attr) in tag.attrs.iter().enumerate() {
            if tag.attrs[..i].iter().any(|a| a.name == attr.name) {
                out.push(diagnostic(
                    attr.name_range.clone(),
                    Severity::Warning,
                    trf!("html.duplicate_attribute", name = attr.name),
                ));
            }
        }
        if let Some(id) = tag.value(text, "id").filter(|v| !v.is_empty()) {
            if ids.contains(&id) {
                let range = tag
                    .attr("id")
                    .and_then(|a| a.value.clone())
                    .unwrap_or_default();
                out.push(diagnostic(
                    range,
                    Severity::Warning,
                    trf!("html.duplicate_id", id = id),
                ));
            } else {
                ids.push(id);
            }
        }
        if tag.name == "img" && tag.attr("alt").is_none() {
            out.push(diagnostic(
                tag.name_range.clone(),
                Severity::Warning,
                tr!("html.image_without_alt").into(),
            ));
        }
    }
    out.sort_by_key(|d| d.range.start);
    out
}

/// `div#main.box.wide`: how an element shows in the structure of the file.
fn label(name: &str, id: Option<&str>, classes: &[&str]) -> String {
    let mut label = name.to_string();
    if let Some(id) = id.filter(|i| !i.is_empty()) {
        label.push('#');
        label.push_str(id);
    }
    for class in classes {
        label.push('.');
        label.push_str(class);
    }
    label
}

pub fn symbols(text: &str) -> Vec<Symbol> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = lexer::scan(text);
    let tree = tree::build(&scan);
    tree.elements
        .iter()
        .map(|element| {
            let tag = &scan.tags[element.open];
            let classes: Vec<&str> = tag
                .value(text, "class")
                .map(|c| c.split_whitespace().collect())
                .unwrap_or_default();
            Symbol {
                name: label(&tag.name, tag.value(text, "id"), &classes),
                kind: ItemKind::Property,
                detail: String::new(),
                range: tag.name_range.clone(),
                depth: element.depth,
            }
        })
        .collect()
}

/// The ids and classes of the document, as (value range, is an id).
fn names(text: &str, scan: &Scan) -> Vec<(Range<usize>, NameKind)> {
    let mut out = Vec::new();
    for tag in scan.tags.iter().filter(|t| !t.closing) {
        for attr in &tag.attrs {
            let Some(value) = attr.value.clone() else {
                continue;
            };
            let kind = if attr.name == "id" {
                NameKind::Id
            } else if attr.name == "class" {
                NameKind::Class
            } else if data::refers_to_id(&attr.name) {
                NameKind::IdReference
            } else if data::is_link(&attr.name) && text[value.clone()].starts_with('#') {
                out.push((value.start + 1..value.end, NameKind::IdReference));
                continue;
            } else {
                continue;
            };
            if kind == NameKind::Id {
                out.push((value, kind));
                continue;
            }
            // Space-separated lists: each name on its own.
            let mut at = value.start;
            for word in text[value.clone()].split(' ') {
                let range = at..at + word.len();
                at += word.len() + 1;
                if !word.trim().is_empty() {
                    let trimmed = range.start + (word.len() - word.trim_start().len())
                        ..range.end - (word.len() - word.trim_end().len());
                    out.push((trimmed, kind));
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NameKind {
    Id,
    IdReference,
    Class,
}

/// The `.name` selectors of the style sheets of the page.
fn class_selectors(text: &str, name: &str) -> Vec<Range<usize>> {
    let regions = embedded::html_regions(text, &lexer::scan(text));
    let mut out = Vec::new();
    for region in regions
        .iter()
        .filter(|r| r.prefix.is_empty() && crate::languages::css::is_style(r.language))
    {
        let css = &text[region.range.clone()];
        let needle = format!(".{name}");
        let mut from = 0;
        while let Some(found) = css[from..].find(&needle) {
            let at = from + found;
            let after = css[at + needle.len()..].chars().next();
            let boundary = !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if boundary {
                let start = region.range.start + at + 1;
                out.push(start..start + name.len());
            }
            from = at + needle.len();
        }
    }
    out
}

pub fn highlights(text: &str, offset: usize) -> Vec<Range<usize>> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = lexer::scan(text);
    // A tag name: its opening and closing tags.
    if let Some((index, tag)) = lexer::tag_at(&scan.tags, offset)
        && tag.name_range.start <= offset
        && offset <= tag.name_range.end
    {
        let tree = tree::build(&scan);
        let element = tree.of_tag[index].or(tree.closed_by[index]);
        return element
            .map(|e| {
                let element = &tree.elements[e];
                let mut ranges = vec![scan.tags[element.open].name_range.clone()];
                if let Some(close) = element.close {
                    ranges.push(scan.tags[close].name_range.clone());
                }
                ranges
            })
            .unwrap_or_default();
    }
    let names = names(text, &scan);
    let Some((range, kind)) = names
        .iter()
        .find(|(r, _)| r.start <= offset && offset <= r.end)
        .cloned()
    else {
        return Vec::new();
    };
    let name = &text[range];
    let ids = matches!(kind, NameKind::Id | NameKind::IdReference);
    let mut out: Vec<Range<usize>> = names
        .into_iter()
        .filter(|(r, k)| {
            &text[r.clone()] == name
                && (if ids {
                    matches!(k, NameKind::Id | NameKind::IdReference)
                } else {
                    *k == NameKind::Class
                })
        })
        .map(|(r, _)| r)
        .collect();
    if !ids {
        out.extend(class_selectors(text, name));
    }
    out.sort_by_key(|r| r.start);
    out
}

pub fn definition(text: &str, offset: usize) -> Vec<Location> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = lexer::scan(text);
    // A closing tag: its opening tag.
    if let Some((index, tag)) = lexer::tag_at(&scan.tags, offset)
        && tag.closing
        && tag.name_range.start <= offset
        && offset <= tag.name_range.end
    {
        let tree = tree::build(&scan);
        return tree.closed_by[index]
            .map(|e| {
                let open = &tree.open_tag(&scan, e).name_range;
                vec![Location::in_text(None, text, open.clone())]
            })
            .unwrap_or_default();
    }
    let names = names(text, &scan);
    let Some((range, kind)) = names
        .iter()
        .find(|(r, _)| r.start <= offset && offset <= r.end)
        .cloned()
    else {
        return Vec::new();
    };
    let name = &text[range];
    match kind {
        NameKind::IdReference => names
            .iter()
            .filter(|(r, k)| *k == NameKind::Id && &text[r.clone()] == name)
            .map(|(r, _)| Location::in_text(None, text, r.clone()))
            .collect(),
        NameKind::Class => class_selectors(text, name)
            .into_iter()
            .map(|r| Location::in_text(None, text, r))
            .collect(),
        NameKind::Id => Vec::new(),
    }
}

/// Where the word being typed at `offset` starts: names of elements and attributes have
/// dashes and colons (`aria-label`, `xlink:href`).
pub fn word_start(text: &str, offset: usize) -> usize {
    let mut from = offset.min(text.len());
    for (i, c) in text[..from].char_indices().rev() {
        if !(c.is_alphanumeric() || matches!(c, '-' | '_' | ':')) {
            break;
        }
        from = i;
    }
    from
}

fn element_item(name: &'static str, insert: String, caret: Option<usize>) -> CompletionItem {
    let mut item = CompletionItem::new(name, ItemKind::Class).insert(insert, caret);
    if let Some(text) = description(name) {
        item = item.doc(text);
    }
    item
}

/// The elements, for a tag being opened. `bare`: only the name goes in, the tag has more.
fn element_items(bare: bool) -> Vec<CompletionItem> {
    data::elements()
        .map(|name| {
            if bare {
                element_item(name, name.into(), None)
            } else if data::is_void(name) {
                element_item(name, format!("{name}>"), None)
            } else {
                // `<div>|</div>`, as IntelliJ completes a tag.
                element_item(name, format!("{name}></{name}>"), Some(name.len() + 1))
            }
        })
        .collect()
}

/// The attributes an element takes, the ones of its own first.
fn attribute_items(element: &str, present: &[&str], bare: bool) -> Vec<CompletionItem> {
    let own = data::own_attributes(element);
    let groups: [(&[&str], u32, &str); 4] = [
        (&own, 10, ""),
        (data::GLOBAL_ATTRIBUTES, 30, tr!("html.global_attribute")),
        (data::ARIA_ATTRIBUTES, 40, "ARIA"),
        (data::EVENT_ATTRIBUTES, 50, tr!("html.event_attribute")),
    ];
    let mut out: Vec<CompletionItem> = Vec::new();
    for (names, rank, detail) in groups {
        for &name in names {
            if present.contains(&name) || out.iter().any(|i| i.label == name) {
                continue;
            }
            let boolean = data::BOOLEAN_ATTRIBUTES.contains(&name);
            let (insert, caret) = if bare || boolean {
                (name.to_string(), None)
            } else {
                (format!("{name}=\"\""), Some(name.len() + 2))
            };
            let detail = if detail.is_empty() {
                format!("<{element}>")
            } else {
                detail.to_string()
            };
            out.push(
                CompletionItem::new(name, ItemKind::Property)
                    .insert(insert, caret)
                    .detail(detail)
                    .rank(rank),
            );
        }
    }
    out
}

/// The values of an attribute: the known ones, the ids for a reference, the classes of the
/// page for `class`.
fn value_items(text: &str, scan: &Scan, element: &str, attribute: &str) -> Vec<CompletionItem> {
    let mut out: Vec<CompletionItem> = data::values(element, attribute)
        .iter()
        .map(|v| CompletionItem::new(*v, ItemKind::Constant))
        .collect();
    let wanted = if attribute == "class" {
        Some(NameKind::Class)
    } else if data::refers_to_id(attribute) || data::is_link(attribute) {
        Some(NameKind::Id)
    } else {
        None
    };
    if let Some(kind) = wanted {
        let mut seen: Vec<String> = Vec::new();
        for (range, k) in names(text, scan) {
            let name = text[range].to_string();
            if k == kind && !name.is_empty() && !seen.contains(&name) {
                let insert = if data::is_link(attribute) {
                    format!("#{name}")
                } else {
                    name.clone()
                };
                out.push(CompletionItem::new(insert, ItemKind::Constant));
                seen.push(name);
            }
        }
    }
    out
}

fn entity_items() -> Vec<CompletionItem> {
    data::ENTITIES
        .iter()
        .map(|(name, character)| {
            CompletionItem::new(*name, ItemKind::Constant)
                .insert(format!("{name};"), None)
                .detail(character.to_string())
        })
        .collect()
}

/// An entity being typed (`&am`): where its name starts.
fn entity_start(text: &str, offset: usize) -> Option<usize> {
    let before = &text[..offset];
    let name_start = before
        .rfind(|c: char| !c.is_ascii_alphanumeric())
        .map_or(0, |i| i + 1);
    (before[..name_start].ends_with('&')).then_some(name_start)
}

pub fn complete(text: &str, offset: usize, explicit: bool) -> Option<Completion> {
    if text.len() > MAX_TEXT || offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let scan = lexer::scan(text);
    if let Some(token) = lexer::token_at(&scan.tokens, offset)
        && token.range.start < offset
        && offset < token.range.end
        && matches!(
            token.kind,
            Kind::Comment | Kind::Declaration | Kind::RawText
        )
    {
        return None;
    }
    let from = word_start(text, offset);
    if let Some((_, tag)) = lexer::tag_at(&scan.tags, offset) {
        // The name of the tag.
        if tag.name_range.start <= offset && offset <= tag.name_range.end {
            let from = tag.name_range.start;
            if tag.closing {
                return Some(Completion {
                    from,
                    items: closing_items(text, &scan, tag.range.start, !tag.complete),
                });
            }
            let bare = tag.complete || !tag.attrs.is_empty();
            return Some(Completion {
                from,
                items: element_items(bare),
            });
        }
        // A value.
        if let Some(attr) = tag.attrs.iter().find(|a| {
            a.value
                .as_ref()
                .is_some_and(|v| v.start <= offset && offset <= v.end)
        }) {
            let value = attr.value.clone().unwrap_or_default();
            let start = text[value.start..offset]
                .rfind(' ')
                .map_or(value.start, |i| value.start + i + 1);
            let items = value_items(text, &scan, &tag.name, &attr.name);
            return (!items.is_empty()).then_some(Completion { from: start, items });
        }
        // The name of an attribute, or the space for one.
        let current = tag
            .attrs
            .iter()
            .find(|a| a.name_range.start <= offset && offset <= a.name_range.end);
        let present: Vec<&str> = tag
            .attrs
            .iter()
            .filter(|a| Some(*a) != current)
            .map(|a| a.name.as_str())
            .collect();
        if tag.closing || (current.is_none() && from != offset && !explicit) {
            return None;
        }
        let bare = current.is_some_and(|a| a.value.is_some());
        return Some(Completion {
            from,
            items: attribute_items(&tag.name, &present, bare),
        });
    }
    // `<` or `</` just typed, with nothing after them yet.
    let before = &text[..from];
    if before.ends_with("</") {
        return Some(Completion {
            from,
            items: closing_items(text, &scan, from - 2, true),
        });
    }
    if before.ends_with('<') {
        return Some(Completion {
            from,
            items: element_items(false),
        });
    }
    if let Some(start) = entity_start(text, offset) {
        return Some(Completion {
            from: start,
            items: entity_items(),
        });
    }
    None
}

/// The elements open at `at`, the innermost first, for a closing tag.
fn closing_items(text: &str, scan: &Scan, at: usize, with_bracket: bool) -> Vec<CompletionItem> {
    let _ = text;
    let tree = tree::build(scan);
    let mut open: Vec<(usize, &str)> = tree
        .elements
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let tag = &scan.tags[e.open];
            tag.range.end <= at
                && (e.close.is_none() || e.end >= at)
                && !data::is_void(&tag.name)
                && !tag.self_closing
        })
        .map(|(i, e)| (e.depth, tree.name(scan, i)))
        .collect();
    open.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
    let mut out: Vec<CompletionItem> = Vec::new();
    for (rank, (_, name)) in open.into_iter().enumerate() {
        if out.iter().any(|i| i.label == name) {
            continue;
        }
        let insert = if with_bracket {
            format!("{name}>")
        } else {
            name.to_string()
        };
        out.push(
            CompletionItem::new(name, ItemKind::Class)
                .insert(insert, None)
                .rank(rank as u32),
        );
    }
    out
}

pub fn hover(text: &str, offset: usize) -> Option<Hover> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let scan = lexer::scan(text);
    if let Some(token) = lexer::token_at(&scan.tokens, offset)
        && token.kind == Kind::Entity
    {
        let entity = &text[token.range.clone()];
        let name = entity.trim_start_matches('&').trim_end_matches(';');
        let character = data::ENTITIES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| c.to_string())
            .or_else(|| {
                let number = name.strip_prefix('#')?;
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => number.parse().ok()?,
                };
                char::from_u32(code).map(String::from)
            })?;
        let code = character.chars().next().map_or(0, |c| c as u32);
        return Some(Hover {
            range: token.range.clone(),
            title: entity.to_string(),
            body: format!("{character}  U+{code:04X}"),
        });
    }
    let (_, tag) = lexer::tag_at(&scan.tags, offset)?;
    if tag.name_range.start <= offset && offset <= tag.name_range.end {
        return Some(Hover {
            range: tag.name_range.clone(),
            title: format!("<{}>", tag.name),
            body: description(&tag.name).unwrap_or_default().to_string(),
        });
    }
    let attr = tag
        .attrs
        .iter()
        .find(|a| a.name_range.start <= offset && offset <= a.name_range.end)?;
    let body = if data::own_attributes(&tag.name).contains(&attr.name.as_str()) {
        trf!("html.attribute_of", element = tag.name)
    } else if data::GLOBAL_ATTRIBUTES.contains(&attr.name.as_str()) {
        tr!("html.global_attribute").to_string()
    } else if attr.name.starts_with("on") {
        tr!("html.event_attribute").to_string()
    } else {
        return None;
    };
    let values = data::values(&tag.name, &attr.name);
    let body = if values.is_empty() {
        body
    } else {
        format!("{body}\n{}", values.join(" | "))
    };
    Some(Hover {
        range: attr.name_range.clone(),
        title: attr.name.clone(),
        body,
    })
}

// ---- Pug ----------------------------------------------------------------------------------

pub fn pug_errors(text: &str) -> Vec<Diagnostic> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    pug::scan(text)
        .problems
        .into_iter()
        .map(|(range, error, message)| {
            let severity = if error {
                Severity::Error
            } else {
                Severity::Warning
            };
            diagnostic(range, severity, message)
        })
        .collect()
}

/// The depth of each tag of a Pug document, from the indentation of their lines.
fn pug_depths(tags: &[pug::PugTag]) -> Vec<usize> {
    let mut stack: Vec<usize> = Vec::new();
    let mut depths = Vec::new();
    for tag in tags {
        if tag.inline {
            depths.push(stack.len());
            continue;
        }
        while stack.last().is_some_and(|&i| i >= tag.indent) {
            stack.pop();
        }
        depths.push(stack.len());
        stack.push(tag.indent);
    }
    depths
}

pub fn pug_symbols(text: &str) -> Vec<Symbol> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = pug::scan(text);
    let depths = pug_depths(&scan.tags);
    let mut out: Vec<Symbol> = scan
        .tags
        .iter()
        .zip(depths)
        .map(|(tag, depth)| {
            let classes: Vec<&str> = tag.classes.iter().map(|r| &text[r.clone()]).collect();
            let range = tag.name_range.clone().unwrap_or_else(|| {
                tag.id
                    .clone()
                    .or_else(|| tag.classes.first().cloned())
                    .unwrap_or(tag.start..tag.start)
            });
            Symbol {
                name: label(&tag.name, tag.id.clone().map(|r| &text[r]), &classes),
                kind: ItemKind::Property,
                detail: String::new(),
                range,
                depth,
            }
        })
        .collect();
    // The mixins, as functions.
    for (range, role) in &scan.tokens {
        if *role == pug::Role::Name && mixin_declaration(text, range.start) {
            out.push(Symbol {
                name: text[range.clone()].to_string(),
                kind: ItemKind::Function,
                detail: "mixin".into(),
                range: range.clone(),
                depth: 0,
            });
        }
    }
    out.sort_by_key(|s| s.range.start);
    out
}

/// Whether the name at `at` follows `mixin ` on its line.
fn mixin_declaration(text: &str, at: usize) -> bool {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..at].trim() == "mixin"
}

/// The name of a mixin at `offset`, declared or called, with its range.
fn mixin_at(text: &str, scan: &pug::Scan, offset: usize) -> Option<(String, Range<usize>)> {
    let (range, _) = scan
        .tokens
        .iter()
        .find(|(r, role)| *role == pug::Role::Name && r.start <= offset && offset <= r.end)?;
    let name = text[range.clone()].trim_start_matches('+').to_string();
    let start = range.end - name.len();
    Some((name, start..range.end))
}

/// Every declaration and call of the mixin `name`.
fn mixin_uses(text: &str, scan: &pug::Scan, name: &str) -> Vec<(Range<usize>, bool)> {
    scan.tokens
        .iter()
        .filter(|(_, role)| *role == pug::Role::Name)
        .filter_map(|(range, _)| {
            let written = &text[range.clone()];
            let declared = mixin_declaration(text, range.start);
            let bare = written.trim_start_matches('+');
            let is_call = written.starts_with('+');
            (bare == name && (declared || is_call))
                .then(|| (range.end - bare.len()..range.end, declared))
        })
        .collect()
}

pub fn pug_highlights(text: &str, offset: usize) -> Vec<Range<usize>> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = pug::scan(text);
    if let Some((name, _)) = mixin_at(text, &scan, offset) {
        return mixin_uses(text, &scan, &name)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
    }
    // An id or a class: everywhere it is written.
    let found = scan.tags.iter().find_map(|tag| {
        tag.id
            .iter()
            .map(|r| (r.clone(), true))
            .chain(tag.classes.iter().map(|r| (r.clone(), false)))
            .find(|(r, _)| r.start <= offset && offset <= r.end)
    });
    let Some((range, id)) = found else {
        return Vec::new();
    };
    let name = &text[range];
    scan.tags
        .iter()
        .flat_map(|tag| {
            if id {
                tag.id.iter().cloned().collect::<Vec<_>>()
            } else {
                tag.classes.clone()
            }
        })
        .filter(|r| &text[r.clone()] == name)
        .collect()
}

pub fn pug_definition(text: &str, offset: usize) -> Vec<Location> {
    if text.len() > MAX_TEXT {
        return Vec::new();
    }
    let scan = pug::scan(text);
    let Some((name, _)) = mixin_at(text, &scan, offset) else {
        return Vec::new();
    };
    mixin_uses(text, &scan, &name)
        .into_iter()
        .filter(|(_, declared)| *declared)
        .map(|(r, _)| Location::in_text(None, text, r))
        .collect()
}

pub fn pug_complete(text: &str, offset: usize, explicit: bool) -> Option<Completion> {
    if text.len() > MAX_TEXT || offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let scan = pug::scan(text);
    let from = word_start(text, offset);
    // Inside the parentheses of a tag: its attributes and their values.
    if let Some(tag) = scan.tags.iter().find(|t| {
        t.attrs_range
            .as_ref()
            .is_some_and(|r| r.start <= offset && offset <= r.end)
    }) {
        if let Some(attr) = tag.attrs.iter().find(|a| {
            a.value
                .as_ref()
                .is_some_and(|v| v.start < offset && offset <= v.end)
        }) {
            let value = attr.value.clone().unwrap_or_default();
            let quoted = text[value.clone()].starts_with(['"', '\'']);
            if !quoted {
                return None;
            }
            let start = text[value.start + 1..offset]
                .rfind(' ')
                .map_or(value.start + 1, |i| value.start + 1 + i + 1);
            let items: Vec<CompletionItem> = data::values(&tag.name, &attr.name)
                .iter()
                .map(|v| CompletionItem::new(*v, ItemKind::Constant))
                .collect();
            return (!items.is_empty()).then_some(Completion { from: start, items });
        }
        let present: Vec<&str> = tag.attrs.iter().map(|a| a.name.as_str()).collect();
        let current = &text[from..offset];
        let present: Vec<&str> = present.into_iter().filter(|p| *p != current).collect();
        let mut items = attribute_items(&tag.name, &present, false);
        for item in &mut items {
            // Pug separates the attributes, and quotes the values the same way.
            item.insert = item.insert.replace("=\"\"", "=''");
        }
        return Some(Completion { from, items });
    }
    let line_start = text[..from].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..from];
    // A mixin call.
    if before.trim_start() == "+" {
        let mut items: Vec<CompletionItem> = Vec::new();
        for (range, role) in &scan.tokens {
            if *role == pug::Role::Name && mixin_declaration(text, range.start) {
                let name = &text[range.clone()];
                if !items.iter().any(|i| i.label == name) {
                    items.push(CompletionItem::new(name, ItemKind::Function).detail("mixin"));
                }
            }
        }
        return (!items.is_empty()).then_some(Completion { from, items });
    }
    // At the start of a line: a tag or a keyword.
    if before.trim().is_empty() && (from != offset || explicit) {
        let mut items: Vec<CompletionItem> = data::elements()
            .map(|name| element_item(name, name.into(), None))
            .collect();
        items.extend(
            pug::KEYWORDS
                .iter()
                .map(|k| CompletionItem::new(*k, ItemKind::Keyword).rank(20)),
        );
        return Some(Completion { from, items });
    }
    None
}

pub fn pug_hover(text: &str, offset: usize) -> Option<Hover> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let scan = pug::scan(text);
    let tag = scan.tags.iter().find(|t| {
        t.name_range
            .as_ref()
            .is_some_and(|r| r.start <= offset && offset <= r.end)
    })?;
    Some(Hover {
        range: tag.name_range.clone().unwrap_or_default(),
        title: format!("<{}>", tag.name),
        body: description(&tag.name).unwrap_or_default().to_string(),
    })
}
