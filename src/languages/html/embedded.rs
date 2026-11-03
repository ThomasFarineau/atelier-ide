//! The code of other languages inside a page: `<script>` (JavaScript, TypeScript), `<style>`
//! (CSS, SCSS, Less, Sass), the `style` attributes and the event handlers (`onclick`), and in
//! Pug the `script.` and `style.` blocks and the JavaScript of its lines.
//!
//! These parts belong to their own language: its colors, and its provider for everything
//! else. The provider is given a document of its language as long as the page, where every
//! byte that is not its code is a space (line breaks kept): the offsets, lines and columns of
//! the page are the ones of that document, so nothing has to be translated back.

use super::{lexer, pug};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub range: Range<usize>,
    /// The language, as `project::language` names it: `js`, `ts`, `css`, `scss`…
    pub language: &'static str,
    /// Its provider analyzes it (a block, an attribute). Pug's inline code is only colored:
    /// its fragments are not a program once put together.
    pub analyzed: bool,
    /// Written over the bytes just before and at the end of the code in the language's
    /// document, so that it reads as code of that language: a `style` attribute is a list of
    /// declarations, `*{` and `}` make it a rule.
    pub prefix: &'static str,
    pub suffix: &'static str,
}

/// The languages whose documents are pages: they hold code of other languages.
pub fn is_markup(language: &str) -> bool {
    matches!(language, "html" | "pug")
}

/// The language of a `<script>` from its `type` and `lang` attributes; None when it holds no
/// code (a JSON block, a template).
pub fn script_language(kind: Option<&str>, lang: Option<&str>) -> Option<&'static str> {
    if let Some(lang) = lang {
        return match lang.trim().to_ascii_lowercase().as_str() {
            "ts" | "typescript" | "tsx" => Some("ts"),
            "js" | "javascript" | "jsx" => Some("js"),
            _ => None,
        };
    }
    match kind.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        None
        | Some("" | "module" | "text/javascript" | "application/javascript")
        | Some("text/ecmascript" | "application/ecmascript" | "text/babel" | "jsx") => Some("js"),
        Some("text/typescript" | "application/typescript" | "ts") => Some("ts"),
        _ => None,
    }
}

/// The language of a `<style>` from its `lang` (or `type`) attribute.
pub fn style_language(kind: Option<&str>, lang: Option<&str>) -> Option<&'static str> {
    let name = lang
        .map(|l| l.trim().to_ascii_lowercase())
        .or_else(|| kind.map(|t| t.trim().to_ascii_lowercase()));
    match name.as_deref() {
        None | Some("" | "css" | "text/css") => Some("css"),
        Some("scss" | "text/scss") => Some("scss"),
        Some("sass" | "text/sass") => Some("sass"),
        Some("less" | "text/less") => Some("less"),
        _ => None,
    }
}

/// The code of other languages in an HTML document.
pub fn html_regions(text: &str, scan: &lexer::Scan) -> Vec<Region> {
    let mut regions = Vec::new();
    for raw in &scan.raw {
        let tag = &scan.tags[raw.tag];
        let language = match tag.name.as_str() {
            "script" => script_language(tag.value(text, "type"), tag.value(text, "lang")),
            "style" => style_language(tag.value(text, "type"), tag.value(text, "lang")),
            _ => None,
        };
        if let Some(language) = language {
            regions.push(Region {
                range: raw.content.clone(),
                language,
                analyzed: true,
                prefix: "",
                suffix: "",
            });
        }
    }
    for tag in scan.tags.iter().filter(|t| !t.closing) {
        for attr in &tag.attrs {
            let Some(value) = attr.value.clone().filter(|_| attr.quoted) else {
                continue;
            };
            // The bytes before the value (`style="`) take the prefix, the closing quote the
            // suffix: there must be room for them.
            let (language, prefix, suffix) = if attr.name == "style" {
                ("css", "*{", "}")
            } else if attr.name.starts_with("on") && attr.name.len() > 2 {
                ("js", ";()=>{", "}")
            } else {
                continue;
            };
            if value.start - attr.name_range.start < prefix.len() || value.end >= text.len() {
                continue;
            }
            regions.push(Region {
                range: value,
                language,
                analyzed: true,
                prefix,
                suffix,
            });
        }
    }
    regions.sort_by_key(|r| r.range.start);
    regions
}

/// The code of other languages in a document of `language`.
pub fn regions(text: &str, language: &str) -> Vec<Region> {
    match language {
        "html" => html_regions(text, &lexer::scan(text)),
        "pug" => pug::scan(text).regions,
        _ => Vec::new(),
    }
}

/// The region that holds `offset`: from its first byte to just after its last.
pub fn region_at(regions: &[Region], offset: usize) -> Option<&Region> {
    regions
        .iter()
        .find(|r| r.range.start <= offset && offset <= r.range.end)
}

/// The languages that have analyzed code in the regions, each once.
pub fn languages(regions: &[Region]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for region in regions.iter().filter(|r| r.analyzed) {
        if !out.contains(&region.language) {
            out.push(region.language);
        }
    }
    out
}

/// The document of `language` made from the page: its code where it is, spaces everywhere
/// else, the line breaks kept.
pub fn document(text: &str, regions: &[Region], language: &str) -> String {
    let mut bytes: Vec<u8> = text
        .bytes()
        .map(|b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();
    for region in regions
        .iter()
        .filter(|r| r.analyzed && r.language == language)
    {
        bytes[region.range.clone()].copy_from_slice(&text.as_bytes()[region.range.clone()]);
        let before = region.range.start.saturating_sub(region.prefix.len());
        let room = before + region.prefix.len() == region.range.start
            && !text[before..region.range.start].contains('\n');
        if room {
            bytes[before..region.range.start].copy_from_slice(region.prefix.as_bytes());
        }
        if !region.suffix.is_empty() && region.range.end + region.suffix.len() <= text.len() {
            let at = region.range.end..region.range.end + region.suffix.len();
            if !text[at.clone()].contains('\n') {
                bytes[at].copy_from_slice(region.suffix.as_bytes());
            }
        }
    }
    // Only ASCII went in over whole characters or over ASCII: still UTF-8.
    String::from_utf8(bytes).unwrap_or_default()
}

/// The language of the code at `offset` in a page: the one of its region, else the page's.
pub fn language_at(text: &str, language: &str, offset: usize) -> String {
    if !is_markup(language) {
        return language.to_string();
    }
    let regions = regions(text, language);
    region_at(&regions, offset)
        .filter(|r| r.analyzed || language == "pug")
        .map_or(language, |r| r.language)
        .to_string()
}

/// A block of code of a page (a `<script>`, a `<style>`), to be formatted on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Its lines in the page: from the start of its first line of code to the end of its last.
    pub body: Range<usize>,
    /// The code without the indentation its lines share.
    pub code: String,
    indent: String,
    crlf: bool,
}

impl Block {
    /// The block of the code at `range`; None when it is empty, or starts on the line of its
    /// tag (`<script>let x;`), which has no indentation of its own to give back.
    pub fn of(page: &str, range: Range<usize>) -> Option<Self> {
        let content = &page[range.clone()];
        let first = content.find(|c: char| !c.is_whitespace())?;
        let line_start = content[..first].rfind('\n')? + 1;
        let end = content.trim_end().len();
        let body = range.start + line_start..range.start + end;
        let text = &page[body.clone()];
        let crlf = text.contains("\r\n");
        let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
        let indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| &l[..l.len() - l.trim_start().len()])
            .min_by_key(|i| i.len())
            .unwrap_or_default()
            .to_string();
        let code = lines
            .iter()
            .map(|l| {
                let own = &l[..l.len() - l.trim_start().len()];
                let cut = if own.starts_with(indent.as_str()) {
                    indent.len()
                } else {
                    own.len()
                };
                &l[cut..]
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(Self {
            body,
            code,
            indent,
            crlf,
        })
    }

    /// Formatted code put back at the indentation of the block.
    pub fn indent(&self, code: &str) -> String {
        code.trim_end()
            .split('\n')
            .map(|l| {
                let l = l.trim_end_matches('\r');
                if l.trim().is_empty() {
                    String::new()
                } else {
                    format!("{}{l}", self.indent)
                }
            })
            .collect::<Vec<_>>()
            .join(if self.crlf { "\r\n" } else { "\n" })
    }
}
