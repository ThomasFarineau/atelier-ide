//! HTML inside Markdown, as GitHub renders it: the blocks (a centered logo, badges, a title) are
//! drawn by a small renderer of the common tags, and the tags inside a paragraph become their
//! Markdown equivalent before the Markdown viewer reads the text.

use crate::ui;
use eframe::egui::{self, Align, RichText, Sense};
use pulldown_cmark::{Event, Options, Parser};
use std::ops::Range;

/// The options of the Markdown viewer: the same text must be seen the same way.
fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_MATH
}

/// A tag, `<a href="x">` or `</a>`.
#[derive(Debug, PartialEq)]
struct Tag {
    name: String,
    closing: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The tag written in `s` (from `<` to `>`); None for a comment, a doctype or a broken tag.
fn parse_tag(s: &str) -> Option<Tag> {
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    let inner = inner.strip_suffix('/').unwrap_or(inner);
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let name = inner[..end].to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let mut attrs = Vec::new();
    let mut rest = inner[end..].trim_start();
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let key = rest[..end].to_ascii_lowercase();
        rest = rest[end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (v, next) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let close = after[1..].find(q).map_or(after.len(), |i| i + 1);
                    (&after[1..close], after.get(close + 1..).unwrap_or(""))
                }
                _ => {
                    let close = after
                        .find(|c: char| c.is_whitespace())
                        .unwrap_or(after.len());
                    (&after[..close], &after[close..])
                }
            };
            value = decode_entities(v);
            rest = next.trim_start();
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
    }
    Some(Tag {
        name,
        closing,
        attrs,
    })
}

/// `&amp;`, `&#39;`… replaced by their character.
pub(super) fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let entity = rest[1..]
            .find(';')
            .filter(|&end| end <= 10)
            .map(|end| &rest[1..end + 1]);
        let decoded = entity.and_then(|e| match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "copy" => Some('©'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            _ => {
                let number = e.strip_prefix('#')?;
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => number.parse().ok()?,
                };
                char::from_u32(code)
            }
        });
        match (entity, decoded) {
            (Some(e), Some(c)) => {
                out.push(c);
                rest = &rest[e.len() + 2..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Markdown text whose inline HTML was replaced by Markdown, and where each replacement was.
pub(super) struct Converted {
    pub text: String,
    /// (range in the source, length of what replaced it), in order.
    replaced: Vec<(Range<usize>, usize)>,
}

impl Converted {
    /// The offset in the source of `offset` in the converted text (outside a replacement).
    pub fn source_offset(&self, offset: usize) -> usize {
        let mut shift: isize = 0;
        for (range, len) in &self.replaced {
            let start = (range.start as isize + shift) as usize;
            if offset < start + len {
                break;
            }
            shift += *len as isize - range.len() as isize;
        }
        (offset as isize - shift) as usize
    }
}

/// The text with the tags of its paragraphs (`<br>`, `<b>`, `<img>`, `<a href>`…) written in
/// Markdown, or None when it has none. HTML blocks are left to [`show`].
pub(super) fn inline_to_markdown(text: &str) -> Option<Converted> {
    let tags: Vec<Range<usize>> = Parser::new_ext(text, parser_options())
        .into_offset_iter()
        .filter(|(event, _)| matches!(event, Event::InlineHtml(_)))
        .map(|(_, range)| range)
        .collect();
    if tags.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut replaced = Vec::new();
    let mut links: Vec<Option<String>> = Vec::new();
    let mut last = 0;
    for range in tags {
        out.push_str(&text[last..range.start]);
        let source = &text[range.clone()];
        let after = &text[range.end..];
        let markdown = match parse_tag(source) {
            None => String::new(),
            Some(tag) => match (tag.name.as_str(), tag.closing) {
                ("br", _) => {
                    // A break at the end of a paragraph would show its backslash.
                    let next_line = after.strip_prefix('\n');
                    let ends = after.trim().is_empty()
                        || next_line
                            .is_some_and(|r| r.trim_start_matches([' ', '\t']).starts_with('\n'));
                    match next_line {
                        _ if ends => String::new(),
                        Some(_) => "\\".into(),
                        None => "\\\n".into(),
                    }
                }
                ("b" | "strong", _) => "**".into(),
                ("i" | "em", _) => "*".into(),
                ("s" | "del" | "strike", _) => "~~".into(),
                ("code" | "kbd" | "tt" | "samp", _) => "`".into(),
                ("img", false) => tag.attr("src").map_or(String::new(), |src| {
                    format!("![{}](<{}>)", tag.attr("alt").unwrap_or(""), src)
                }),
                ("a", false) => {
                    let href = tag.attr("href").map(str::to_string);
                    let open = if href.is_some() { "[" } else { "" };
                    links.push(href);
                    open.into()
                }
                ("a", true) => match links.pop().flatten() {
                    Some(href) => format!("](<{href}>)"),
                    None => String::new(),
                },
                _ => String::new(),
            },
        };
        replaced.push((range.clone(), markdown.len()));
        out.push_str(&markdown);
        last = range.end;
    }
    out.push_str(&text[last..]);
    Some(Converted {
        text: out,
        replaced,
    })
}

/// A piece of a line of an HTML block.
#[derive(Debug, Clone, PartialEq)]
enum Inline {
    Text {
        text: String,
        strong: bool,
        italic: bool,
        code: bool,
        strike: bool,
        link: Option<String>,
    },
    Image {
        src: String,
        alt: String,
        width: Option<f32>,
        height: Option<f32>,
        link: Option<String>,
    },
    Break,
}

#[derive(Debug, PartialEq)]
enum Line {
    Rule,
    Content {
        align: Align,
        heading: Option<u8>,
        indent: usize,
        items: Vec<Inline>,
    },
}

#[derive(Default)]
struct Builder {
    lines: Vec<Line>,
    items: Vec<Inline>,
    aligns: Vec<(String, Align)>,
    heading: Option<u8>,
    strong: usize,
    italic: usize,
    code: usize,
    strike: usize,
    pre: usize,
    hidden: usize,
    links: Vec<Option<String>>,
    /// Lists being written: the number of the next item, None for a bulleted list.
    lists: Vec<Option<usize>>,
    /// The cells written in the current table row.
    cells: usize,
}

impl Builder {
    fn align(&self) -> Align {
        self.aligns.last().map_or(Align::Min, |(_, a)| *a)
    }

    fn flush(&mut self) {
        while matches!(self.items.last(), Some(Inline::Break)) {
            self.items.pop();
        }
        if let Some(Inline::Text { text, .. }) = self.items.last_mut() {
            let trimmed = text.trim_end().len();
            text.truncate(trimmed);
        }
        if self.items.is_empty() {
            return;
        }
        self.lines.push(Line::Content {
            align: self.align(),
            heading: self.heading,
            indent: self.lists.len(),
            items: std::mem::take(&mut self.items),
        });
    }

    fn at_line_start(&self) -> bool {
        match self.items.last() {
            None | Some(Inline::Break) => true,
            Some(Inline::Text { text, .. }) => text.ends_with(' ') || text.ends_with('\n'),
            Some(Inline::Image { .. }) => false,
        }
    }

    fn text(&mut self, raw: &str) {
        if self.hidden > 0 {
            return;
        }
        let decoded = decode_entities(raw);
        let text = if self.pre > 0 {
            decoded
        } else {
            let mut collapsed = String::with_capacity(decoded.len());
            for c in decoded.chars() {
                if c.is_whitespace() && c != '\u{a0}' {
                    if !collapsed.ends_with(' ') {
                        collapsed.push(' ');
                    }
                } else {
                    collapsed.push(c);
                }
            }
            if self.at_line_start() {
                collapsed.trim_start().to_string()
            } else {
                collapsed
            }
        };
        if text.is_empty() {
            return;
        }
        let item = Inline::Text {
            text,
            strong: self.strong > 0 || self.heading.is_some(),
            italic: self.italic > 0,
            code: self.code > 0 || self.pre > 0,
            strike: self.strike > 0,
            link: self.links.iter().rev().find_map(Clone::clone),
        };
        // Pieces of the same style make one label, wrapped as a whole.
        if let (
            Some(Inline::Text {
                text: last,
                strong,
                italic,
                code,
                strike,
                link,
            }),
            Inline::Text {
                text: new,
                strong: s,
                italic: i,
                code: c,
                strike: k,
                link: l,
            },
        ) = (self.items.last_mut(), &item)
            && (*strong, *italic, *code, *strike, link.as_deref()) == (*s, *i, *c, *k, l.as_deref())
        {
            last.push_str(new);
            return;
        }
        self.items.push(item);
    }

    fn tag(&mut self, tag: Tag) {
        let name = tag.name.as_str();
        if matches!(name, "script" | "style" | "template") {
            if tag.closing {
                self.hidden = self.hidden.saturating_sub(1);
            } else {
                self.hidden += 1;
            }
            return;
        }
        if self.hidden > 0 {
            return;
        }
        let block = matches!(
            name,
            "p" | "div"
                | "center"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "ul"
                | "ol"
                | "li"
                | "pre"
                | "blockquote"
                | "details"
                | "summary"
                | "section"
                | "header"
                | "footer"
                | "table"
                | "thead"
                | "tbody"
                | "tr"
                | "dl"
                | "dt"
                | "dd"
                | "figure"
                | "figcaption"
        );
        if block {
            self.flush();
        }
        let toggle = |count: &mut usize| {
            if tag.closing {
                *count = count.saturating_sub(1);
            } else {
                *count += 1;
            }
        };
        match name {
            "b" | "strong" | "th" | "summary" | "dt" => toggle(&mut self.strong),
            "i" | "em" | "cite" => toggle(&mut self.italic),
            "code" | "kbd" | "tt" | "samp" => toggle(&mut self.code),
            "s" | "del" | "strike" => toggle(&mut self.strike),
            "pre" => toggle(&mut self.pre),
            _ => {}
        }
        match (name, tag.closing) {
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                self.heading = name[1..].parse().ok();
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => self.heading = None,
            ("br", _) => self.items.push(Inline::Break),
            ("hr", _) => {
                self.flush();
                self.lines.push(Line::Rule);
            }
            ("a", false) => self.links.push(tag.attr("href").map(str::to_string)),
            ("a", true) => {
                self.links.pop();
            }
            ("img", false) => {
                let size = |name: &str| {
                    tag.attr(name)
                        .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
                };
                if let Some(src) = tag.attr("src") {
                    self.items.push(Inline::Image {
                        src: src.to_string(),
                        alt: tag.attr("alt").unwrap_or("").to_string(),
                        width: size("width"),
                        height: size("height"),
                        link: self.links.iter().rev().find_map(Clone::clone),
                    });
                }
            }
            ("ul", false) => self.lists.push(None),
            ("ol", false) => self.lists.push(Some(
                tag.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1),
            )),
            ("ul" | "ol", true) => {
                self.lists.pop();
            }
            ("li", false) => {
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => "• ".into(),
                };
                self.text(&marker);
            }
            ("tr", false) => self.cells = 0,
            ("td" | "th", false) => {
                if self.cells > 0 {
                    self.text(" \u{a0}\u{a0}");
                }
                self.cells += 1;
            }
            _ => {}
        }
        if block && !tag.closing {
            let align = match tag.attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("center") => Some(Align::Center),
                Some("right") => Some(Align::Max),
                Some("left") => Some(Align::Min),
                _ if name == "center" => Some(Align::Center),
                _ => None,
            };
            if let Some(align) = align {
                self.aligns.push((tag.name.clone(), align));
            }
        } else if block
            && tag.closing
            && let Some(at) = self.aligns.iter().rposition(|(n, _)| *n == tag.name)
        {
            self.aligns.truncate(at);
        }
    }
}

/// The lines of an HTML block.
fn layout(html: &str) -> Vec<Line> {
    let mut builder = Builder::default();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        let tag = rest
            .strip_prefix('<')
            .filter(|r| r.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!'))
            .and_then(|_| rest.find('>'));
        match tag {
            Some(end) => {
                if let Some(tag) = parse_tag(&rest[..=end]) {
                    builder.tag(tag);
                }
                rest = &rest[end + 1..];
            }
            None => {
                let end = rest[1..].find('<').map_or(rest.len(), |i| i + 1);
                builder.text(&rest[..end]);
                rest = &rest[end..];
            }
        }
    }
    builder.flush();
    builder.lines
}

/// The address of an image or a link relative to the document.
fn resolve(src: &str, base: &str) -> String {
    if src.contains("://") || src.starts_with("data:") || src.starts_with('#') {
        src.to_string()
    } else {
        format!("{base}{}", src.trim_start_matches("./"))
    }
}

/// Draws an HTML block of a Markdown document. `base` is the folder of the document (the base
/// of relative images), `max_width` the width of the widest image.
pub(super) fn show(ui: &mut egui::Ui, html: &str, base: &str, max_width: f32) {
    let t = ui::theme(ui.ctx());
    for (index, line) in layout(html).into_iter().enumerate() {
        let Line::Content {
            align,
            heading,
            indent,
            items,
        } = line
        else {
            ui.separator();
            continue;
        };
        let size = match heading {
            Some(1) => 28.0,
            Some(2) => 22.0,
            Some(3) => 18.0,
            Some(4) => 16.0,
            _ => 14.0,
        };
        // A centered line is placed from the width it had at the last frame.
        let id = ui.id().with(("html_line", index));
        let width = ui.data(|d| d.get_temp::<f32>(id));
        let available = ui.available_width();
        let offset = match (align, width) {
            (Align::Center, Some(w)) => ((available - w) / 2.0).max(0.0),
            (Align::Max, Some(w)) => (available - w).max(0.0),
            _ => 0.0,
        } + indent as f32 * 18.0;
        let response = ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(offset);
            for item in &items {
                match item {
                    Inline::Break => ui.end_row(),
                    Inline::Text {
                        text,
                        strong,
                        italic,
                        code,
                        strike,
                        link,
                    } => {
                        let mut rich = RichText::new(text).size(size);
                        if *strong {
                            rich = rich.font(ui::semibold(size));
                        }
                        if *italic {
                            rich = rich.italics();
                        }
                        if *code {
                            rich = rich.code();
                        }
                        if *strike {
                            rich = rich.strikethrough();
                        }
                        match link {
                            Some(href) => {
                                ui.hyperlink_to(rich.color(t.link), resolve(href, base));
                            }
                            None => {
                                ui.label(rich);
                            }
                        }
                    }
                    Inline::Image {
                        src,
                        alt,
                        width,
                        height,
                        link,
                    } => {
                        let uri = resolve(src, base);
                        let mut image = egui::Image::new(uri.clone())
                            .max_width(width.unwrap_or(max_width).min(max_width));
                        if let Some(h) = height {
                            image = image.max_height(*h);
                        }
                        let failed = image.load_for_size(ui.ctx(), ui.available_size()).is_err();
                        let response = if failed {
                            // An image that cannot be loaded (a remote badge) shows its text.
                            if alt.is_empty() {
                                continue;
                            }
                            ui.label(RichText::new(alt).size(size).color(t.text_secondary))
                        } else {
                            ui.add(image.sense(Sense::click()))
                        };
                        if let Some(href) = link {
                            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if response.clicked() {
                                ui.ctx()
                                    .open_url(egui::OpenUrl::new_tab(resolve(href, base)));
                            }
                        }
                    }
                }
            }
        });
        let measured = response.response.rect.width() - offset;
        if width.is_none_or(|w| (w - measured).abs() > 0.5) && align != Align::Min {
            ui.data_mut(|d| d.insert_temp(id, measured));
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/markdown/html/tests.rs"]
mod tests;
