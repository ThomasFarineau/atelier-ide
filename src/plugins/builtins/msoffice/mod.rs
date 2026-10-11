//! MSOfficeViewer built-in plugin: Microsoft Office documents open in a read-only viewer, and
//! export to PDF. Word (`.docx`) documents are drawn as a page, with headings, character
//! formatting, lists, links, tables and images; Excel workbooks (`.xlsx`) as a grid with one tab
//! per sheet (`xlsx`); PowerPoint presentations (`.pptx`) as a column of slides (`pptx`). These
//! formats are zips of XML files, read here without any office suite, so the plugin behaves the
//! same on every OS. The PDF files are written by `pdf`, with no external tool.

mod pdf;
mod pptx;
mod word_pdf;
mod xlsx;

use crate::{
    plugins::builtin::{BuiltinPlugin, Command, EditorLayout, Host},
    ui,
};
use anyhow::{Context as _, Result, anyhow, bail};
use eframe::egui::{
    self, Align, Color32, ColorImage, FontId, Layout, RichText, Shape, Stroke, StrokeKind,
    TextFormat, TextureHandle, TextureOptions, text::LayoutJob, vec2,
};
use quick_xml::{Reader, events::Event};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Largest file opened, in bytes.
const MAX_FILE: u64 = 128 * 1024 * 1024;
/// Largest XML part or image read from the archive once decompressed.
const MAX_PART: u64 = 64 * 1024 * 1024;
const MAX_IMAGES_TOTAL: usize = 192 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 200;
/// Largest side of an image texture.
const MAX_SIDE: u32 = 2048;

const INK: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const LINK: Color32 = Color32::from_rgb(0x05, 0x63, 0xC1);
const PAPER_BORDER: Color32 = Color32::from_rgb(0xBF, 0xBF, 0xBF);

pub fn is_word(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "docx" | "docm" | "dotx" | "dotm" | "doc"
        )
    })
}

pub fn is_office(path: &Path) -> bool {
    is_word(path) || xlsx::is_sheet(path) || pptx::is_slides(path)
}

/// `c` with its luminance multiplied by `multiply` and then raised by `add` (0 to 1), keeping
/// its hue and saturation: the `lumMod`/`lumOff` and tints of Office colors.
fn lum(c: Color32, multiply: f32, add: f32) -> Color32 {
    let [r, g, b] = [c.r(), c.g(), c.b()].map(|v| f32::from(v) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (l, d) = ((max + min) / 2.0, max - min);
    let denominator = 1.0 - (2.0 * l - 1.0).abs();
    let (h, s) = if d <= f32::EPSILON || denominator <= 1e-6 {
        (0.0, 0.0)
    } else {
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h, d / denominator)
    };
    let l = (l * multiply + add).clamp(0.0, 1.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = l - chroma / 2.0;
    let channel = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(channel(r), channel(g), channel(b))
}

// ---- XML tree ---------------------------------------------------------------------------

/// An XML element without namespaces: `w:p` is `p`, `w:val` is `val`.
#[derive(Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    /// First descendant with this name, depth first.
    fn find(&self, name: &str) -> Option<&Node> {
        self.children.iter().find_map(|c| {
            if c.name == name {
                Some(c)
            } else {
                c.find(name)
            }
        })
    }
    fn val(&self, child: &str) -> Option<&str> {
        self.child(child).and_then(|c| c.attr("val"))
    }
}

fn local(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    name.rsplit(':').next().unwrap_or("").to_string()
}

fn parse_xml(bytes: &[u8]) -> Result<Node> {
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();
    let mut stack = vec![Node::default()];
    let element = |e: &quick_xml::events::BytesStart| -> Result<Node> {
        let mut node = Node {
            name: local(e.name().as_ref()),
            ..Node::default()
        };
        for attr in e.attributes().flatten() {
            node.attrs.push((
                local(attr.key.as_ref()),
                attr.unescape_value()?.into_owned(),
            ));
        }
        Ok(node)
    };
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                if stack.len() > MAX_XML_DEPTH {
                    bail!(tr!("msoffice.document_nested_too_deeply"));
                }
                stack.push(element(&e)?);
            }
            Event::Empty(e) => {
                let node = element(&e)?;
                stack.last_mut().expect("root").children.push(node);
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    bail!(tr!("msoffice.malformed_xml"));
                }
                let node = stack.pop().expect("node");
                stack.last_mut().expect("root").children.push(node);
            }
            Event::Text(t) => stack
                .last_mut()
                .expect("root")
                .text
                .push_str(&t.unescape()?),
            Event::CData(c) => stack
                .last_mut()
                .expect("root")
                .text
                .push_str(&String::from_utf8_lossy(&c)),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if stack.len() != 1 {
        bail!(tr!("msoffice.truncated_xml"));
    }
    stack
        .pop()
        .and_then(|mut root| root.children.pop())
        .ok_or_else(|| anyhow!(tr!("msoffice.empty_xml")))
}

// ---- Document model ---------------------------------------------------------------------

#[derive(Clone, Default, PartialEq)]
struct Format {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<Color32>,
    /// Size in points.
    size: Option<f32>,
    sup: bool,
    sub: bool,
    highlight: Option<Color32>,
}

enum Inline {
    Text {
        text: String,
        format: Format,
        link: Option<String>,
    },
    Image {
        rel: String,
        /// Size in pixels from the document, if it gives one.
        size: Option<[f32; 2]>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Body,
    Title,
    Heading(u8),
}

struct Para {
    kind: Kind,
    align: Align,
    /// Left indent in points.
    indent: f32,
    marker: Option<String>,
    inlines: Vec<Inline>,
}

struct Cell {
    span: usize,
    fill: Option<Color32>,
    blocks: Vec<Block>,
}

struct Table {
    /// Relative width of the grid columns.
    widths: Vec<f32>,
    rows: Vec<Vec<Cell>>,
}

enum Block {
    Para(Para),
    Table(Table),
}

struct Doc {
    blocks: Vec<Block>,
    /// Image data by relationship id.
    images: HashMap<String, Vec<u8>>,
    words: usize,
}

struct Rel {
    target: String,
    external: bool,
    image: bool,
}

#[derive(Default)]
struct Style {
    kind: Option<Kind>,
    num: Option<(String, usize)>,
}

#[derive(Default)]
struct Level {
    format: String,
    text: String,
    start: u32,
}

#[derive(Default)]
struct Ctx {
    rels: HashMap<String, Rel>,
    styles: HashMap<String, Style>,
    /// numId -> abstractNumId
    nums: HashMap<String, String>,
    /// abstractNumId -> levels
    abstracts: HashMap<String, Vec<Level>>,
    counters: HashMap<String, Vec<u32>>,
}

fn hex_color(text: &str) -> Option<Color32> {
    if text.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    Some(Color32::from_rgb(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

fn highlight_color(name: &str) -> Option<Color32> {
    Some(match name {
        "yellow" => Color32::from_rgb(255, 255, 0),
        "green" => Color32::from_rgb(0, 255, 0),
        "cyan" => Color32::from_rgb(0, 255, 255),
        "magenta" => Color32::from_rgb(255, 0, 255),
        "blue" => Color32::from_rgb(120, 120, 255),
        "red" => Color32::from_rgb(255, 90, 90),
        "lightGray" => Color32::from_rgb(211, 211, 211),
        _ => return None,
    })
}

fn flag(node: &Node, name: &str) -> bool {
    node.child(name)
        .is_some_and(|c| !matches!(c.attr("val"), Some("0" | "false" | "off" | "none")))
}

fn roman(mut n: u32, upper: bool) -> String {
    const PARTS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, text) in PARTS {
        while n >= value {
            out.push_str(text);
            n -= value;
        }
    }
    if upper { out.to_uppercase() } else { out }
}

fn letters(n: u32, upper: bool) -> String {
    let mut n = n.max(1) - 1;
    let mut out = Vec::new();
    loop {
        out.push((b'a' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    let text: String = out.into_iter().rev().collect();
    if upper { text.to_uppercase() } else { text }
}

fn number(format: &str, n: u32) -> String {
    match format {
        "lowerLetter" => letters(n, false),
        "upperLetter" => letters(n, true),
        "lowerRoman" => roman(n, false),
        "upperRoman" => roman(n, true),
        _ => n.to_string(),
    }
}

fn kind_of_style(name: &str) -> Option<Kind> {
    let name = name.to_lowercase();
    if name == "title" {
        return Some(Kind::Title);
    }
    name.strip_prefix("heading ")
        .and_then(|n| n.trim().parse::<u8>().ok())
        .map(|n| Kind::Heading(n.clamp(1, 9)))
}

impl Ctx {
    fn read_styles(&mut self, root: &Node) {
        for style in root.children.iter().filter(|c| c.name == "style") {
            let Some(id) = style.attr("styleId") else {
                continue;
            };
            let kind = style
                .val("name")
                .and_then(kind_of_style)
                .or_else(|| kind_of_style(id.replace(char::is_numeric, " ").trim()))
                .or_else(|| {
                    let lower = id.to_lowercase();
                    lower
                        .strip_prefix("heading")
                        .and_then(|n| n.parse::<u8>().ok())
                        .map(|n| Kind::Heading(n.clamp(1, 9)))
                });
            let num = style
                .child("pPr")
                .and_then(|p| p.child("numPr"))
                .and_then(|n| {
                    let id = n.val("numId")?.to_string();
                    let level = n.val("ilvl").and_then(|l| l.parse().ok()).unwrap_or(0);
                    Some((id, level))
                });
            self.styles.insert(id.to_string(), Style { kind, num });
        }
    }

    fn read_numbering(&mut self, root: &Node) {
        for abs in root.children.iter().filter(|c| c.name == "abstractNum") {
            let Some(id) = abs.attr("abstractNumId") else {
                continue;
            };
            let mut levels = Vec::new();
            for lvl in abs.children.iter().filter(|c| c.name == "lvl") {
                let index: usize = lvl
                    .attr("ilvl")
                    .and_then(|i| i.parse().ok())
                    .unwrap_or(levels.len());
                if levels.len() <= index {
                    levels.resize_with(index + 1, Level::default);
                }
                levels[index] = Level {
                    format: lvl.val("numFmt").unwrap_or("decimal").to_string(),
                    text: lvl.val("lvlText").unwrap_or("").to_string(),
                    start: lvl.val("start").and_then(|s| s.parse().ok()).unwrap_or(1),
                };
            }
            self.abstracts.insert(id.to_string(), levels);
        }
        for num in root.children.iter().filter(|c| c.name == "num") {
            if let (Some(id), Some(abs)) = (num.attr("numId"), num.val("abstractNumId")) {
                self.nums.insert(id.to_string(), abs.to_string());
            }
        }
    }

    /// The marker of the next item of the list `num_id` at `level`: a bullet or a number.
    fn marker(&mut self, num_id: &str, level: usize) -> String {
        const BULLETS: [&str; 3] = ["•", "◦", "▪"];
        let levels = self.nums.get(num_id).and_then(|a| self.abstracts.get(a));
        let Some(def) = levels.and_then(|l| l.get(level)) else {
            return BULLETS[level % 3].into();
        };
        if def.format == "bullet" || def.format == "none" {
            return BULLETS[level % 3].into();
        }
        let start = def.start;
        let counters = self.counters.entry(num_id.to_string()).or_default();
        if counters.len() <= level {
            counters.resize(level + 1, 0);
        }
        counters[level] = if counters[level] == 0 {
            start
        } else {
            counters[level] + 1
        };
        counters.truncate(level + 1);
        let counters = counters.clone();
        let levels = self
            .nums
            .get(num_id)
            .and_then(|a| self.abstracts.get(a))
            .expect("known list");
        let mut text = def.text.clone();
        if text.is_empty() {
            text = format!("%{}.", level + 1);
        }
        for (k, count) in counters.iter().enumerate() {
            let fmt = levels.get(k).map_or("decimal", |l| l.format.as_str());
            text = text.replace(&format!("%{}", k + 1), &number(fmt, (*count).max(1)));
        }
        text
    }
}

fn format_of(rpr: Option<&Node>) -> Format {
    let Some(rpr) = rpr else {
        return Format::default();
    };
    Format {
        bold: flag(rpr, "b"),
        italic: flag(rpr, "i"),
        underline: flag(rpr, "u"),
        strike: flag(rpr, "strike") || flag(rpr, "dstrike"),
        color: rpr
            .val("color")
            .filter(|c| *c != "auto")
            .and_then(hex_color),
        size: rpr
            .val("sz")
            .and_then(|s| s.parse::<f32>().ok())
            .map(|s| s / 2.0),
        sup: rpr.val("vertAlign") == Some("superscript"),
        sub: rpr.val("vertAlign") == Some("subscript"),
        highlight: rpr.val("highlight").and_then(highlight_color).or_else(|| {
            rpr.child("shd")
                .and_then(|s| s.attr("fill"))
                .filter(|f| *f != "auto")
                .and_then(hex_color)
        }),
    }
}

fn push_text(out: &mut Vec<Inline>, text: &str, format: &Format, link: Option<&str>) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text {
        text: last,
        format: last_format,
        link: last_link,
    }) = out.last_mut()
        && last_format == format
        && last_link.as_deref() == link
    {
        last.push_str(text);
        return;
    }
    out.push(Inline::Text {
        text: text.to_string(),
        format: format.clone(),
        link: link.map(str::to_string),
    });
}

fn run_content(node: &Node, format: &Format, link: Option<&str>, out: &mut Vec<Inline>) {
    for c in &node.children {
        match c.name.as_str() {
            "t" => push_text(out, &c.text, format, link),
            "tab" => push_text(out, "\t", format, link),
            "br" | "cr" => push_text(out, "\n", format, link),
            "noBreakHyphen" => push_text(out, "\u{2011}", format, link),
            "drawing" => {
                if let Some(rel) = c.find("blip").and_then(|b| b.attr("embed")) {
                    let size = c.find("extent").and_then(|e| {
                        let cx: f32 = e.attr("cx")?.parse().ok()?;
                        let cy: f32 = e.attr("cy")?.parse().ok()?;
                        // 9525 EMU per pixel.
                        Some([cx / 9525.0, cy / 9525.0])
                    });
                    out.push(Inline::Image {
                        rel: rel.to_string(),
                        size,
                    });
                }
            }
            "pict" => {
                if let Some(rel) = c.find("imagedata").and_then(|b| b.attr("id")) {
                    out.push(Inline::Image {
                        rel: rel.to_string(),
                        size: None,
                    });
                }
            }
            // Only the modern branch: the fallback would repeat the same picture.
            "AlternateContent" => {
                if let Some(choice) = c.child("Choice") {
                    run_content(choice, format, link, out);
                }
            }
            _ => {}
        }
    }
}

fn inlines_of(node: &Node, ctx: &Ctx, link: Option<&str>, out: &mut Vec<Inline>) {
    for c in &node.children {
        match c.name.as_str() {
            "r" => {
                let format = format_of(c.child("rPr"));
                run_content(c, &format, link, out);
            }
            "hyperlink" => {
                let target = c
                    .attr("id")
                    .and_then(|id| ctx.rels.get(id))
                    .filter(|r| r.external)
                    .map(|r| r.target.clone());
                inlines_of(c, ctx, target.as_deref().or(link), out);
            }
            "ins" | "smartTag" | "sdt" | "sdtContent" | "fldSimple" | "customXml" | "moveTo" => {
                inlines_of(c, ctx, link, out);
            }
            _ => {}
        }
    }
}

fn paragraph(p: &Node, ctx: &mut Ctx) -> Para {
    let ppr = p.child("pPr");
    let style = ppr
        .and_then(|p| p.val("pStyle"))
        .and_then(|id| ctx.styles.get(id));
    let mut kind = style.and_then(|s| s.kind).unwrap_or(Kind::Body);
    if kind == Kind::Body
        && let Some(level) = ppr
            .and_then(|p| p.val("outlineLvl"))
            .and_then(|l| l.parse::<u8>().ok())
        && level < 9
    {
        kind = Kind::Heading(level + 1);
    }
    let own_num = ppr.and_then(|p| p.child("numPr")).map(|n| {
        (
            n.val("numId").unwrap_or("0").to_string(),
            n.val("ilvl").and_then(|l| l.parse().ok()).unwrap_or(0usize),
        )
    });
    let num = own_num
        .or_else(|| style.and_then(|s| s.num.clone()))
        .filter(|(id, _)| id != "0");
    let marker = num.as_ref().map(|(id, level)| ctx.marker(id, *level));
    let mut indent = ppr
        .and_then(|p| p.child("ind"))
        .and_then(|i| i.attr("left").or_else(|| i.attr("start")))
        .and_then(|l| l.parse::<f32>().ok())
        .map_or(0.0, |twips| (twips / 20.0).clamp(0.0, 200.0));
    if let Some((_, level)) = &num {
        indent = indent.max(18.0 * (*level as f32 + 1.0));
    }
    let align = match ppr.and_then(|p| p.val("jc")) {
        Some("center") => Align::Center,
        Some("right" | "end") => Align::Max,
        _ => Align::Min,
    };
    let mut inlines = Vec::new();
    inlines_of(p, ctx, None, &mut inlines);
    Para {
        kind,
        align,
        indent,
        marker,
        inlines,
    }
}

fn table(tbl: &Node, ctx: &mut Ctx) -> Table {
    let widths = tbl
        .child("tblGrid")
        .map(|g| {
            g.children
                .iter()
                .filter(|c| c.name == "gridCol")
                .map(|c| {
                    c.attr("w")
                        .and_then(|w| w.parse::<f32>().ok())
                        .unwrap_or(1.0)
                        .max(1.0)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    for tr in tbl.children.iter().filter(|c| c.name == "tr") {
        let mut cells = Vec::new();
        for tc in tr.children.iter().filter(|c| c.name == "tc") {
            let pr = tc.child("tcPr");
            let span = pr
                .and_then(|p| p.val("gridSpan"))
                .and_then(|s| s.parse().ok())
                .unwrap_or(1usize)
                .clamp(1, 64);
            let merged = pr
                .and_then(|p| p.child("vMerge"))
                .is_some_and(|m| m.attr("val") != Some("restart"));
            let fill = pr
                .and_then(|p| p.child("shd"))
                .and_then(|s| s.attr("fill"))
                .and_then(hex_color);
            let blocks = if merged {
                Vec::new()
            } else {
                blocks_of(tc, ctx)
            };
            cells.push(Cell { span, fill, blocks });
        }
        rows.push(cells);
    }
    Table { widths, rows }
}

fn blocks_of(node: &Node, ctx: &mut Ctx) -> Vec<Block> {
    let mut blocks = Vec::new();
    for c in &node.children {
        match c.name.as_str() {
            "p" => blocks.push(Block::Para(paragraph(c, ctx))),
            "tbl" => blocks.push(Block::Table(table(c, ctx))),
            "sdt" => {
                if let Some(content) = c.child("sdtContent") {
                    blocks.extend(blocks_of(content, ctx));
                }
            }
            _ => {}
        }
    }
    blocks
}

fn count_words(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Para(p) => p
                .inlines
                .iter()
                .map(|i| match i {
                    Inline::Text { text, .. } => text.split_whitespace().count(),
                    Inline::Image { .. } => 0,
                })
                .sum::<usize>(),
            Block::Table(t) => t
                .rows
                .iter()
                .flatten()
                .map(|c| count_words(&c.blocks))
                .sum::<usize>(),
        })
        .sum::<usize>()
}

fn read_part<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    let file = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut data = Vec::new();
    file.take(MAX_PART + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_PART {
        bail!(trf!("msoffice.too_large", name = name));
    }
    Ok(Some(data))
}

fn load_doc(path: &Path) -> Result<Doc> {
    let size = fs::metadata(path)?.len();
    if size > MAX_FILE {
        bail!(tr!("msoffice.document_too_large_limit_128"));
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("doc"))
    {
        bail!(tr!("msoffice.binary_doc_format_not_supported"));
    }
    let file = fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|_| anyhow!(tr!("msoffice.file_not_valid_word_document")))?;
    let document = read_part(&mut zip, "word/document.xml")?
        .ok_or_else(|| anyhow!(tr!("msoffice.file_not_word_document_word")))?;
    let mut ctx = Ctx::default();
    if let Some(bytes) = read_part(&mut zip, "word/_rels/document.xml.rels")? {
        let rels = parse_xml(&bytes).context(tr!("msoffice.relations_document"))?;
        for rel in rels.children.iter().filter(|c| c.name == "Relationship") {
            if let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) {
                ctx.rels.insert(
                    id.to_string(),
                    Rel {
                        target: target.to_string(),
                        external: rel.attr("TargetMode") == Some("External"),
                        image: rel.attr("Type").is_some_and(|t| t.ends_with("/image")),
                    },
                );
            }
        }
    }
    // A broken style or numbering part only costs the formatting.
    if let Some(root) = read_part(&mut zip, "word/styles.xml")?.and_then(|b| parse_xml(&b).ok()) {
        ctx.read_styles(&root);
    }
    if let Some(root) = read_part(&mut zip, "word/numbering.xml")?.and_then(|b| parse_xml(&b).ok())
    {
        ctx.read_numbering(&root);
    }
    let root = parse_xml(&document).context("word/document.xml")?;
    let body = root
        .child("body")
        .ok_or_else(|| anyhow!(tr!("msoffice.document_without_body")))?;
    let blocks = blocks_of(body, &mut ctx);

    let mut images = HashMap::new();
    let mut total = 0usize;
    for (id, rel) in ctx.rels.iter().filter(|(_, r)| r.image && !r.external) {
        let target = rel.target.trim_start_matches('/');
        let name = if target.starts_with("word/") {
            target.to_string()
        } else {
            format!("word/{target}")
        };
        if let Some(data) = read_part(&mut zip, &name)? {
            total += data.len();
            if total > MAX_IMAGES_TOTAL {
                break;
            }
            images.insert(id.clone(), data);
        }
    }
    let words = count_words(&blocks);
    Ok(Doc {
        blocks,
        images,
        words,
    })
}

// ---- Drawing ----------------------------------------------------------------------------

type Textures = HashMap<String, Option<(TextureHandle, [f32; 2])>>;

struct Env<'a> {
    scale: f32,
    images: &'a HashMap<String, Vec<u8>>,
    textures: &'a mut Textures,
}

impl Env<'_> {
    /// Points to screen pixels.
    fn px(&self, points: f32) -> f32 {
        points * 1.36 * self.scale
    }
}

fn base_points(kind: Kind) -> f32 {
    match kind {
        Kind::Title => 26.0,
        Kind::Heading(1) => 20.0,
        Kind::Heading(2) => 16.0,
        Kind::Heading(3) => 14.0,
        Kind::Heading(_) => 12.5,
        Kind::Body => 11.0,
    }
}

fn text_format(env: &Env, kind: Kind, format: &Format, link: bool) -> TextFormat {
    let mut points = format.size.unwrap_or_else(|| base_points(kind));
    if format.sup || format.sub {
        points *= 0.65;
    }
    let size = env.px(points);
    let bold = format.bold || kind != Kind::Body;
    let color = if link {
        LINK
    } else {
        format.color.unwrap_or(INK)
    };
    TextFormat {
        font_id: if bold {
            ui::semibold(size)
        } else {
            FontId::proportional(size)
        },
        color,
        background: format.highlight.unwrap_or(Color32::TRANSPARENT),
        italics: format.italic,
        underline: if format.underline || link {
            Stroke::new(1.0_f32, color)
        } else {
            Stroke::NONE
        },
        strikethrough: if format.strike {
            Stroke::new(1.0_f32, color)
        } else {
            Stroke::NONE
        },
        valign: if format.sup {
            Align::TOP
        } else if format.sub {
            Align::BOTTOM
        } else {
            Align::Center
        },
        ..Default::default()
    }
}

fn decode_image(ctx: &egui::Context, rel: &str, data: &[u8]) -> Option<(TextureHandle, [f32; 2])> {
    let image = image::load_from_memory(data).ok()?;
    let original = [image.width() as f32, image.height() as f32];
    let image = if image.width().max(image.height()) > MAX_SIDE {
        image.thumbnail(MAX_SIDE, MAX_SIDE)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let color = ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Some((
        ctx.load_texture(format!("docx:{rel}"), color, TextureOptions::LINEAR),
        original,
    ))
}

fn draw_image(ui: &mut egui::Ui, env: &mut Env, rel: &str, size: Option<[f32; 2]>) {
    let ctx = ui.ctx().clone();
    let images = env.images;
    let entry = env
        .textures
        .entry(rel.to_string())
        .or_insert_with(|| images.get(rel).and_then(|d| decode_image(&ctx, rel, d)));
    let Some((texture, natural)) = entry else {
        ui.label(RichText::new("[image]").color(PAPER_BORDER));
        return;
    };
    let wanted = size.unwrap_or(*natural);
    let mut size = vec2(wanted[0], wanted[1]) * env.scale;
    if size.x <= 0.0 || size.y <= 0.0 {
        size = vec2(natural[0], natural[1]) * env.scale;
    }
    let fit = (ui.available_width() / size.x)
        .min(1400.0 * env.scale / size.y)
        .min(1.0);
    ui.add(egui::Image::new((texture.id(), size * fit)));
}

fn draw_para(ui: &mut egui::Ui, p: &Para, env: &mut Env) {
    let heading = p.kind != Kind::Body;
    if heading {
        ui.add_space(env.px(8.0));
    }
    let content = |ui: &mut egui::Ui, env: &mut Env| {
        if p.inlines.is_empty() {
            ui.add_space(env.px(base_points(p.kind)) * 1.25);
            return;
        }
        let special = p.inlines.iter().any(|i| match i {
            Inline::Image { .. } => true,
            Inline::Text { link, .. } => link.is_some(),
        });
        if !special {
            let mut job = LayoutJob::default();
            for inline in &p.inlines {
                if let Inline::Text { text, format, .. } = inline {
                    job.append(
                        &text.replace('\t', "    "),
                        0.0,
                        text_format(env, p.kind, format, false),
                    );
                }
            }
            ui.with_layout(Layout::top_down(p.align), |ui| {
                ui.add(egui::Label::new(job).wrap());
            });
        } else if p.inlines.iter().all(|i| matches!(i, Inline::Image { .. })) {
            ui.with_layout(Layout::top_down(p.align), |ui| {
                for inline in &p.inlines {
                    if let Inline::Image { rel, size } = inline {
                        draw_image(ui, env, rel, *size);
                    }
                }
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.visuals_mut().hyperlink_color = LINK;
                for inline in &p.inlines {
                    match inline {
                        Inline::Image { rel, size } => draw_image(ui, env, rel, *size),
                        Inline::Text { text, format, link } => {
                            let tf = text_format(env, p.kind, format, link.is_some());
                            let mut rich = RichText::new(text.replace('\t', "    "))
                                .font(tf.font_id.clone())
                                .color(tf.color);
                            if format.italic {
                                rich = rich.italics();
                            }
                            if format.underline {
                                rich = rich.underline();
                            }
                            if format.strike {
                                rich = rich.strikethrough();
                            }
                            if format.sup {
                                rich = rich.raised();
                            }
                            if tf.background != Color32::TRANSPARENT {
                                rich = rich.background_color(tf.background);
                            }
                            match link {
                                Some(url) => {
                                    ui.hyperlink_to(rich, url);
                                }
                                None => {
                                    ui.label(rich);
                                }
                            }
                        }
                    }
                }
            });
        }
    };
    if p.indent > 0.0 || p.marker.is_some() {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let indent = env.px(p.indent);
            let marker_width = env.px(16.0);
            ui.add_space(
                (indent
                    - if p.marker.is_some() {
                        marker_width
                    } else {
                        0.0
                    })
                .max(0.0),
            );
            if let Some(marker) = &p.marker {
                let format = Format::default();
                let tf = text_format(env, p.kind, &format, false);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(marker_width, env.px(14.0)), egui::Sense::hover());
                ui.painter().text(
                    rect.right_top() - vec2(env.px(3.0), 0.0),
                    egui::Align2::RIGHT_TOP,
                    marker,
                    tf.font_id,
                    INK,
                );
            }
            ui.vertical(|ui| content(ui, env));
        });
    } else {
        content(ui, env);
    }
    ui.add_space(env.px(if heading { 4.0 } else { 6.0 }));
}

fn draw_blocks(ui: &mut egui::Ui, blocks: &[Block], env: &mut Env) {
    for block in blocks {
        match block {
            Block::Para(p) => draw_para(ui, p, env),
            Block::Table(t) => draw_table(ui, t, env),
        }
    }
}

fn draw_table(ui: &mut egui::Ui, t: &Table, env: &mut Env) {
    let columns = t
        .rows
        .iter()
        .map(|r| r.iter().map(|c| c.span).sum::<usize>())
        .max()
        .unwrap_or(0)
        .max(t.widths.len());
    if columns == 0 {
        return;
    }
    let total = ui.available_width();
    let mut relative: Vec<f32> = t.widths.clone();
    relative.resize(
        columns,
        relative.iter().sum::<f32>() / relative.len().max(1) as f32,
    );
    if relative.iter().any(|w| *w <= 0.0) {
        relative = vec![1.0; columns];
    }
    let sum: f32 = relative.iter().sum();
    let widths: Vec<f32> = relative.iter().map(|w| total * w / sum).collect();
    let pad = env.px(4.0);
    for row in &t.rows {
        let top = ui.cursor().top();
        let mut cells = Vec::new();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut column = 0;
            for cell in row {
                let end = (column + cell.span).min(columns);
                let width: f32 = widths[column.min(columns)..end].iter().sum();
                column = end;
                let fill_slot = ui.painter().add(Shape::Noop);
                let response = ui.allocate_ui_with_layout(
                    vec2(width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                            ui.set_width((width - 2.0 * pad).max(8.0));
                            draw_blocks(ui, &cell.blocks, env);
                        });
                    },
                );
                cells.push((response.response.rect, fill_slot, cell.fill));
            }
        });
        let height = cells.iter().map(|(r, _, _)| r.height()).fold(0.0, f32::max);
        for (rect, slot, fill) in cells {
            let rect =
                egui::Rect::from_min_size(egui::pos2(rect.left(), top), vec2(rect.width(), height));
            if let Some(fill) = fill {
                ui.painter().set(slot, Shape::rect_filled(rect, 0.0, fill));
            }
            ui.painter().rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0_f32, PAPER_BORDER),
                StrokeKind::Inside,
            );
        }
    }
    ui.add_space(env.px(6.0));
}

enum Document {
    Word(Doc),
    Sheet(xlsx::Workbook),
    Slides(pptx::Deck),
}

fn load_document(path: &Path) -> Result<Document> {
    if is_word(path) {
        load_doc(path).map(Document::Word)
    } else if xlsx::is_sheet(path) {
        xlsx::load(path).map(Document::Sheet)
    } else {
        pptx::load(path).map(Document::Slides)
    }
}

/// Writes the document at `path` as a PDF file.
fn convert_to_pdf(path: &Path, target: &Path) -> Result<()> {
    match load_document(path)? {
        Document::Word(doc) => word_pdf::export(&doc, target),
        Document::Sheet(book) => xlsx::export_pdf(&book, target),
        Document::Slides(deck) => pptx::export_pdf(&deck, target),
    }
}

struct Loaded {
    modified: Option<SystemTime>,
    doc: Result<Document, String>,
    textures: Textures,
    /// Height of each top-level block of a Word document from the last frame, 0 when unknown.
    heights: Vec<f32>,
    /// What the heights were measured for: page width and zoom.
    measured_for: (f32, f32),
    sheet: xlsx::SheetView,
}

pub struct MsOfficeViewerPlugin {
    loaded: HashMap<PathBuf, Loaded>,
    zoom: f32,
    /// Outcome of the last PDF export: the document, whether it failed, and what to tell.
    message: Option<(PathBuf, bool, String)>,
}

impl Default for MsOfficeViewerPlugin {
    fn default() -> Self {
        Self {
            loaded: HashMap::new(),
            zoom: 1.0,
            message: None,
        }
    }
}

impl MsOfficeViewerPlugin {
    /// Asks where to save, then writes the PDF. The outcome is kept in `message` and returned.
    fn export(&mut self, path: &Path) -> Option<String> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        let mut dialog = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.pdf"))
            .add_filter(tr!("msoffice.pdf_document"), &["pdf"]);
        if let Some(dir) = path.parent() {
            dialog = dialog.set_directory(dir);
        }
        let target = dialog.save_file()?;
        let (failed, text) = match convert_to_pdf(path, &target) {
            Ok(()) => (false, trf!("msoffice.exported", target = target.display())),
            Err(e) => (
                true,
                trf!("msoffice.error_cannot_export_pdf", e = format!("{:#}", e)),
            ),
        };
        self.message = Some((path.to_path_buf(), failed, text.clone()));
        Some(text)
    }
}

impl BuiltinPlugin for MsOfficeViewerPlugin {
    fn id(&self) -> &'static str {
        "msofficeviewer"
    }
    fn name(&self) -> &'static str {
        "MSOfficeViewer"
    }
    fn description(&self) -> &'static str {
        tr!("msoffice.read_only_viewer_microsoft_office")
    }
    fn icon(&self) -> &'static str {
        "msoffice/icon"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("msoffice")
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command {
            id: "msoffice.pdf",
            label: tr!("msoffice.msoffice_export_document_as_pdf").into(),
        }]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        if id != "msoffice.pdf" {
            return;
        }
        match host.active_file().filter(|p| is_office(p)) {
            Some(path) => {
                if let Some(text) = self.export(&path) {
                    host.notify(text);
                }
            }
            None => host.notify(tr!("msoffice.open_word_excel_powerpoint_document").into()),
        }
    }

    fn opens_binary(&self, path: &Path) -> bool {
        is_office(path)
    }
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_office(path).then_some(EditorLayout::Preview)
    }
    fn layout_switch(&self, _path: &Path) -> bool {
        false
    }

    fn preview(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _text: &mut String,
        _scroll: Option<f32>,
    ) {
        let t = ui::theme(ui.ctx());
        let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
        if self.loaded.get(path).is_none_or(|l| l.modified != modified) {
            let doc = load_document(path).map_err(|e| format!("{e:#}"));
            let blocks = match &doc {
                Ok(Document::Word(d)) => d.blocks.len(),
                _ => 0,
            };
            self.loaded.insert(
                path.to_path_buf(),
                Loaded {
                    modified,
                    doc,
                    textures: HashMap::new(),
                    heights: vec![0.0; blocks],
                    measured_for: (0.0, 0.0),
                    sheet: xlsx::SheetView::default(),
                },
            );
        }
        let zoom = &mut self.zoom;
        let loaded = self.loaded.get_mut(path).expect("just loaded");
        let Loaded {
            doc,
            textures,
            heights,
            measured_for,
            sheet,
            ..
        } = loaded;
        let document = match doc {
            Ok(document) => document,
            Err(error) => {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(&*error).color(t.error)));
                return;
            }
        };

        let summary = match document {
            Document::Word(doc) => trf!("msoffice.read_only_words", words = doc.words),
            Document::Sheet(book) => trf!(
                "msoffice.read_only_sheet_cells",
                count = book.sheets.len(),
                plural = if book.sheets.len() > 1 { "s" } else { "" },
                book = book.cell_count()
            ),
            Document::Slides(deck) => trf!(
                "msoffice.read_only_slide_words",
                count = deck.slides.len(),
                plural = if deck.slides.len() > 1 { "s" } else { "" },
                words = deck.words
            ),
        };
        let mut export = false;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(summary).small().color(t.text_secondary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                if ui::icon_button(ui, "add", tr!("msoffice.zoom")).clicked() {
                    *zoom = (*zoom + 0.1).min(2.5);
                }
                if ui
                    .add(
                        egui::Label::new(RichText::new(format!("{:.0} %", *zoom * 100.0)).small())
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text(tr!("msoffice.actual_size"))
                    .clicked()
                {
                    *zoom = 1.0;
                }
                if ui::icon_button(ui, "remove", tr!("msoffice.zoom_out")).clicked() {
                    *zoom = (*zoom - 0.1).max(0.4);
                }
                ui.add_space(8.0);
                export = ui
                    .button(tr!("msoffice.export_pdf"))
                    .on_hover_text(tr!("msoffice.save_document_as_pdf"))
                    .clicked();
            });
        });
        if let Some((_, failed, text)) = self.message.as_ref().filter(|(p, _, _)| p == path) {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(text).small().color(if *failed {
                    t.error
                } else {
                    t.text_secondary
                }));
            });
        }
        ui.add_space(4.0);

        let scale = *zoom;
        match document {
            Document::Sheet(book) => xlsx::show(ui, book, sheet, path, scale),
            Document::Slides(deck) => pptx::show(ui, deck, textures, path, scale),
            Document::Word(doc) => {
                egui::ScrollArea::vertical()
                    .id_salt(("docx", path))
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, viewport| {
                        ui.vertical_centered(|ui| {
                            let paper = (ui.available_width() - 32.0).clamp(240.0, 816.0 * scale);
                            let margin = (56.0 * scale).min(paper / 6.0);
                            let key = (paper.round(), scale);
                            if *measured_for != key {
                                heights.iter_mut().for_each(|h| *h = 0.0);
                                *measured_for = key;
                            }
                            ui.set_width(paper);
                            ui.add_space(16.0);
                            egui::Frame::new()
                                .fill(Color32::WHITE)
                                .stroke(Stroke::new(1.0_f32, t.border))
                                .inner_margin(margin)
                                .show(ui, |ui| {
                                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                        ui.set_width(paper - 2.0 * margin);
                                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                                        let mut env = Env {
                                            scale,
                                            images: &doc.images,
                                            textures,
                                        };
                                        let mut y = 16.0 + margin;
                                        for (i, block) in doc.blocks.iter().enumerate() {
                                            let known = heights[i];
                                            let near = y + known >= viewport.min.y - 600.0
                                                && y <= viewport.max.y + 600.0;
                                            if known > 0.0 && !near {
                                                ui.add_space(known);
                                                y += known;
                                                continue;
                                            }
                                            if known == 0.0 && y > viewport.max.y + 1500.0 {
                                                // Not measured and far below: a guess, replaced when it is drawn.
                                                ui.add_space(40.0);
                                                y += 40.0;
                                                continue;
                                            }
                                            let before = ui.cursor().top();
                                            match block {
                                                Block::Para(p) => draw_para(ui, p, &mut env),
                                                Block::Table(table) => {
                                                    draw_table(ui, table, &mut env)
                                                }
                                            }
                                            let height = ui.cursor().top() - before;
                                            heights[i] = height;
                                            y += height;
                                        }
                                    });
                                });
                            ui.add_space(24.0);
                        });
                    });
            }
        }
        if export {
            self.export(path);
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/msoffice/mod/tests.rs"]
mod tests;
