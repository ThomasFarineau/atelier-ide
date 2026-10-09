//! PowerPoint presentations (`.pptx`): slides drawn as they are laid out in the file, with
//! shapes, text, pictures and tables, and exported to PDF one page per slide. Text, positions,
//! colors and pictures follow the file; effects, animations and charts are not drawn.

use super::{
    INK, MAX_FILE, MAX_IMAGES_TOTAL, Node, Textures, decode_image, hex_color, lum, parse_xml, read_part,
    pdf::{Font, Inl, Pdf, Span, flow},
};
use crate::ui;
use anyhow::{Context as _, Result, anyhow, bail};
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextFormat, Vec2,
    pos2, text::LayoutJob, vec2,
};
use std::{collections::HashMap, fs, path::Path};

const EMU_PT: f32 = 12700.0;
const MAX_SLIDES: usize = 1000;
const DEFAULT_SIZE: f32 = 18.0;

pub fn is_slides(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "pptx" | "pptm" | "potx" | "potm" | "ppsx" | "ppsm" | "ppt" | "pps"
        )
    })
}

// ---- Model ------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Color32,
    /// Size in points.
    pub size: f32,
}

#[derive(Clone, Debug)]
pub struct TextPara {
    pub align: Align,
    pub bullet: bool,
    /// Left margin in EMU.
    pub margin: f32,
    /// Size of an empty paragraph, in points.
    pub size: f32,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug)]
pub struct Text {
    pub paras: Vec<TextPara>,
    pub anchor: Anchor,
    /// Left, top, right and bottom insets in EMU.
    pub insets: [f32; 4],
    /// Font scale of the "shrink text on overflow" option.
    pub scale: f32,
    pub wrap: bool,
}

impl Text {
    fn is_blank(&self) -> bool {
        self.paras.iter().all(|p| p.runs.iter().all(|r| r.text.trim().is_empty()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Geom {
    Rect,
    Round,
    Ellipse,
}

pub struct TableCell {
    pub fill: Option<Color32>,
    pub text: Text,
    /// Columns covered, 0 for a cell hidden under a merged one.
    pub span: usize,
}

pub struct TableRow {
    pub height: f32,
    pub cells: Vec<TableCell>,
}

pub enum Item {
    Shape {
        rect: Rect,
        geom: Geom,
        fill: Option<Color32>,
        line: Option<(Color32, f32)>,
        text: Option<Text>,
    },
    Line {
        a: Pos2,
        b: Pos2,
        color: Color32,
        width: f32,
    },
    Image {
        rect: Rect,
        name: String,
        /// Cropped fractions: left, top, right, bottom.
        crop: [f32; 4],
    },
    Table {
        rect: Rect,
        columns: Vec<f32>,
        rows: Vec<TableRow>,
    },
}

pub struct Slide {
    pub bg: Color32,
    pub items: Vec<Item>,
}

pub struct Deck {
    /// Slide size in EMU.
    pub size: Vec2,
    pub slides: Vec<Slide>,
    pub media: HashMap<String, Vec<u8>>,
    pub words: usize,
}

// ---- Reading ----------------------------------------------------------------------------

type Rels = HashMap<String, (String, String)>;

fn num(node: &Node, key: &str) -> Option<f32> {
    node.attr(key)?.parse().ok()
}

/// Path of a part referenced from `part`.
fn resolve(part: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let dir = part.rsplit_once('/').map_or("", |(d, _)| d);
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn read_rels<R: std::io::Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, part: &str) -> Rels {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let path = format!("{dir}/_rels/{file}.rels");
    let Some(root) = read_part(zip, &path).ok().flatten().and_then(|b| parse_xml(&b).ok()) else {
        return Rels::new();
    };
    root.children
        .iter()
        .filter(|c| c.name == "Relationship" && c.attr("TargetMode") != Some("External"))
        .filter_map(|rel| {
            Some((
                rel.attr("Id")?.to_string(),
                (resolve(part, rel.attr("Target")?), rel.attr("Type").unwrap_or("").to_string()),
            ))
        })
        .collect()
}

struct Theme(HashMap<String, Color32>);

impl Theme {
    fn read(root: &Node) -> Self {
        let mut colors = HashMap::new();
        if let Some(scheme) = root.find("clrScheme") {
            for c in &scheme.children {
                let Some(inner) = c.children.first() else { continue };
                let value = inner
                    .attr("val")
                    .filter(|_| inner.name == "srgbClr")
                    .or_else(|| inner.attr("lastClr"))
                    .and_then(hex_color);
                if let Some(value) = value {
                    colors.insert(c.name.clone(), value);
                }
            }
        }
        Self(colors)
    }
    fn get(&self, name: &str) -> Option<Color32> {
        let name = match name {
            "tx1" => "dk1",
            "bg1" => "lt1",
            "tx2" => "dk2",
            "bg2" => "lt2",
            other => other,
        };
        self.0.get(name).copied().or(match name {
            "dk1" => Some(Color32::BLACK),
            "lt1" => Some(Color32::WHITE),
            _ => None,
        })
    }
}

/// The color described by an element such as `srgbClr` or `schemeClr` and its modifiers.
fn color_of(node: &Node, theme: &Theme) -> Option<Color32> {
    let base = match node.name.as_str() {
        "srgbClr" => hex_color(node.attr("val")?)?,
        "schemeClr" => theme.get(node.attr("val")?)?,
        "sysClr" => node.attr("lastClr").and_then(hex_color).unwrap_or(Color32::BLACK),
        "prstClr" => match node.attr("val")? {
            "white" => Color32::WHITE,
            "red" => Color32::RED,
            "blue" => Color32::BLUE,
            "green" => Color32::GREEN,
            "yellow" => Color32::YELLOW,
            _ => Color32::BLACK,
        },
        _ => return None,
    };
    let (mut multiply, mut add) = (1.0f32, 0.0f32);
    for modifier in &node.children {
        let Some(v) = num(modifier, "val").map(|v| v / 100_000.0) else {
            continue;
        };
        match modifier.name.as_str() {
            "lumMod" | "shade" => multiply *= v,
            "lumOff" => add += v,
            "tint" => {
                multiply *= v;
                add += 1.0 - v;
            }
            _ => {}
        }
    }
    Some(if multiply != 1.0 || add != 0.0 { lum(base, multiply, add) } else { base })
}

/// The solid color of the `solidFill` child of `node`.
fn solid_fill(node: &Node, theme: &Theme) -> Option<Color32> {
    node.child("solidFill")?.children.first().and_then(|c| color_of(c, theme))
}

enum Fill {
    /// Explicitly empty.
    Empty,
    Solid(Color32),
    /// Not said: the style of the shape decides.
    Unset,
}

fn fill_of(node: &Node, theme: &Theme) -> Fill {
    for c in &node.children {
        match c.name.as_str() {
            "noFill" => return Fill::Empty,
            "solidFill" => return c.children.first().and_then(|c| color_of(c, theme)).map_or(Fill::Unset, Fill::Solid),
            "gradFill" => {
                let first = c.child("gsLst").and_then(|l| l.children.first()).and_then(|g| g.children.first());
                return first.and_then(|g| color_of(g, theme)).map_or(Fill::Unset, Fill::Solid);
            }
            "pattFill" => {
                return c
                    .child("fgClr")
                    .and_then(|f| f.children.first())
                    .and_then(|f| color_of(f, theme))
                    .map_or(Fill::Unset, Fill::Solid);
            }
            _ => {}
        }
    }
    Fill::Unset
}

#[derive(Clone, Copy)]
struct Transform {
    ox: f32,
    oy: f32,
    sx: f32,
    sy: f32,
}

impl Transform {
    const IDENTITY: Self = Self { ox: 0.0, oy: 0.0, sx: 1.0, sy: 1.0 };
    fn rect(self, r: Rect) -> Rect {
        Rect::from_min_size(
            pos2(self.ox + r.min.x * self.sx, self.oy + r.min.y * self.sy),
            vec2(r.width() * self.sx, r.height() * self.sy),
        )
    }
}

/// Position and size of the element holding an `xfrm`, and whether it is flipped.
fn xfrm_of(holder: &Node) -> Option<(Rect, bool, bool)> {
    let xf = holder.child("xfrm")?;
    let off = xf.child("off")?;
    let ext = xf.child("ext")?;
    let rect = Rect::from_min_size(
        pos2(num(off, "x")?, num(off, "y")?),
        vec2(num(ext, "cx")?.max(0.0), num(ext, "cy")?.max(0.0)),
    );
    Some((rect, xf.attr("flipH") == Some("1"), xf.attr("flipV") == Some("1")))
}

#[derive(Clone)]
struct PhKey {
    kind: Option<String>,
    idx: Option<String>,
}

fn ph_of(shape: &Node) -> Option<PhKey> {
    let nv = shape.children.iter().find(|c| c.name.starts_with("nv"))?;
    let ph = nv.child("nvPr")?.child("ph")?;
    Some(PhKey {
        kind: ph.attr("type").map(str::to_string),
        idx: ph.attr("idx").map(str::to_string),
    })
}

fn norm(kind: Option<&str>, master: bool) -> &str {
    match kind {
        Some("ctrTitle" | "title") => "title",
        Some("subTitle") if !master => "subTitle",
        Some("body" | "obj" | "subTitle") | None => "body",
        Some(other) => other,
    }
}

fn align_of(text: &str) -> Option<Align> {
    match text {
        "l" | "just" => Some(Align::Min),
        "ctr" => Some(Align::Center),
        "r" => Some(Align::Max),
        _ => None,
    }
}

#[derive(Default)]
struct Props {
    size: Option<f32>,
    bullet: Option<bool>,
    align: Option<Align>,
    color: Option<Color32>,
}

fn paragraph_props(ppr: &Node, theme: &Theme) -> Props {
    let def = ppr.child("defRPr");
    Props {
        size: def.and_then(|d| num(d, "sz")).map(|s| s / 100.0),
        bullet: if ppr.child("buNone").is_some() {
            Some(false)
        } else if ["buChar", "buAutoNum", "buBlip"].iter().any(|b| ppr.child(b).is_some()) {
            Some(true)
        } else {
            None
        },
        align: ppr.attr("algn").and_then(align_of),
        color: def.and_then(|d| solid_fill(d, theme)),
    }
}

struct Parser<'a> {
    theme: &'a Theme,
    layout: &'a Node,
    master: &'a Node,
}

impl Parser<'_> {
    fn find_ph<'n>(tree: &'n Node, key: &PhKey, master: bool) -> Option<&'n Node> {
        let tree = tree.find("spTree")?;
        let shapes = || tree.children.iter().filter(|c| c.name == "sp");
        if let (Some(idx), false) = (&key.idx, master) {
            if let Some(found) = shapes().find(|sp| ph_of(sp).is_some_and(|k| k.idx.as_ref() == Some(idx))) {
                return Some(found);
            }
        }
        let wanted = norm(key.kind.as_deref(), master);
        shapes().find(|sp| ph_of(sp).is_some_and(|k| norm(k.kind.as_deref(), master) == wanted))
    }

    fn placeholders(&self, key: &PhKey) -> [Option<&Node>; 2] {
        [Self::find_ph(self.layout, key, false), Self::find_ph(self.master, key, true)]
    }

    fn inherited_rect(&self, key: &PhKey) -> Option<(Rect, bool, bool)> {
        self.placeholders(key)
            .into_iter()
            .flatten()
            .find_map(|sp| sp.child("spPr").and_then(xfrm_of))
    }

    /// Text properties from the layout, the master and the master text styles.
    fn props(&self, key: Option<&PhKey>, level: usize) -> Props {
        let name = format!("lvl{}pPr", level + 1);
        let mut sources: Vec<&Node> = Vec::new();
        let style = match key.map(|k| norm(k.kind.as_deref(), false)) {
            Some("title") => "titleStyle",
            Some("dt" | "ftr" | "sldNum") | None => "otherStyle",
            Some(_) => "bodyStyle",
        };
        if let Some(key) = key {
            for sp in self.placeholders(key).into_iter().flatten() {
                if let Some(p) = sp.child("txBody").and_then(|t| t.child("lstStyle")).and_then(|l| l.child(&name)) {
                    sources.push(p);
                }
            }
        }
        if let Some(p) = self.master.child("txStyles").and_then(|t| t.child(style)).and_then(|s| s.child(&name)) {
            sources.push(p);
        }
        let mut out = Props::default();
        for p in sources {
            let props = paragraph_props(p, self.theme);
            out.size = out.size.or(props.size);
            out.bullet = out.bullet.or(props.bullet);
            out.align = out.align.or(props.align);
            out.color = out.color.or(props.color);
        }
        out
    }

    fn anchor(&self, body_pr: Option<&Node>, key: Option<&PhKey>) -> Anchor {
        let own = body_pr.and_then(|b| b.attr("anchor"));
        let inherited = key.and_then(|k| {
            self.placeholders(k)
                .into_iter()
                .flatten()
                .find_map(|sp| sp.child("txBody")?.child("bodyPr")?.attr("anchor"))
        });
        match own.or(inherited) {
            Some("ctr") => Anchor::Middle,
            Some("b") => Anchor::Bottom,
            Some(_) => Anchor::Top,
            None if key.is_some_and(|k| norm(k.kind.as_deref(), false) == "title") => Anchor::Middle,
            None => Anchor::Top,
        }
    }

    fn text(&self, body: &Node, key: Option<&PhKey>, shape_color: Option<Color32>, insets: [f32; 4]) -> Text {
        let body_pr = body.child("bodyPr");
        let inset = |name: &str, default: f32| body_pr.and_then(|b| num(b, name)).unwrap_or(default);
        let scale = body_pr
            .and_then(|b| b.child("normAutofit"))
            .and_then(|n| num(n, "fontScale"))
            .map_or(1.0, |s| s / 100_000.0);
        let mut paras = Vec::new();
        for p in body.children.iter().filter(|c| c.name == "p") {
            let ppr = p.child("pPr");
            let level = ppr.and_then(|p| num(p, "lvl")).map_or(0, |l| l as usize).min(8);
            let inherited = self.props(key, level);
            let own = ppr.map(|p| paragraph_props(p, self.theme)).unwrap_or_default();
            let bullet = own.bullet.or(inherited.bullet).unwrap_or(false);
            let default_color = shape_color
                .or(inherited.color)
                .unwrap_or_else(|| self.theme.get("tx1").unwrap_or(INK));
            let default_size = own.size.or(inherited.size).unwrap_or(DEFAULT_SIZE);
            let mut runs = Vec::new();
            for c in &p.children {
                let (rpr, text) = match c.name.as_str() {
                    "r" => (c.child("rPr"), c.child("t").map(|t| t.text.clone())),
                    "fld" => (c.child("rPr"), c.child("t").map(|t| t.text.clone())),
                    "br" => (c.child("rPr"), Some("\n".to_string())),
                    _ => continue,
                };
                let Some(text) = text.filter(|t| !t.is_empty()) else { continue };
                let flag = |name: &str| rpr.and_then(|r| r.attr(name)).is_some_and(|v| v == "1" || v == "true");
                runs.push(Run {
                    text,
                    bold: flag("b"),
                    italic: flag("i"),
                    underline: rpr.and_then(|r| r.attr("u")).is_some_and(|u| u != "none"),
                    color: rpr.and_then(|r| solid_fill(r, self.theme)).unwrap_or(default_color),
                    size: rpr.and_then(|r| num(r, "sz")).map_or(default_size, |s| s / 100.0),
                });
            }
            let end_size = p.child("endParaRPr").and_then(|r| num(r, "sz")).map(|s| s / 100.0);
            paras.push(TextPara {
                align: own.align.or(inherited.align).unwrap_or(Align::Min),
                bullet,
                margin: ppr
                    .and_then(|p| num(p, "marL"))
                    .unwrap_or(if bullet { 342_900.0 * (level as f32 + 1.0) } else { 0.0 }),
                size: end_size.unwrap_or(default_size),
                runs,
            });
        }
        Text {
            paras,
            anchor: self.anchor(body_pr, key),
            insets: [
                inset("lIns", insets[0]),
                inset("tIns", insets[1]),
                inset("rIns", insets[2]),
                inset("bIns", insets[3]),
            ],
            scale,
            wrap: body_pr.and_then(|b| b.attr("wrap")) != Some("none"),
        }
    }

    fn walk(&self, tree: &Node, rels: &Rels, tf: Transform, skip_ph: bool, depth: usize, out: &mut Vec<Item>) {
        for c in &tree.children {
            match c.name.as_str() {
                "sp" | "cxnSp" => self.shape(c, tf, skip_ph, out),
                "pic" => self.picture(c, rels, tf, skip_ph, out),
                "graphicFrame" => self.frame(c, tf, out),
                "grpSp" if depth < 8 => {
                    let Some((group, _, _)) = c.child("grpSpPr").and_then(xfrm_of) else {
                        continue;
                    };
                    let child = c
                        .child("grpSpPr")
                        .and_then(|g| g.child("xfrm"))
                        .and_then(|x| Some((x.child("chOff")?, x.child("chExt")?)));
                    let placed = tf.rect(group);
                    let inner = match child {
                        Some((off, ext)) => {
                            let (cx, cy) = (num(off, "x").unwrap_or(0.0), num(off, "y").unwrap_or(0.0));
                            let (w, h) = (num(ext, "cx").unwrap_or(0.0), num(ext, "cy").unwrap_or(0.0));
                            let sx = if w > 0.0 { placed.width() / w } else { 1.0 };
                            let sy = if h > 0.0 { placed.height() / h } else { 1.0 };
                            Transform {
                                ox: placed.min.x - cx * sx,
                                oy: placed.min.y - cy * sy,
                                sx,
                                sy,
                            }
                        }
                        None => tf,
                    };
                    self.walk(c, rels, inner, skip_ph, depth + 1, out);
                }
                _ => {}
            }
        }
    }

    fn style_color(&self, shape: &Node, reference: &str) -> Option<Color32> {
        let r = shape.child("style")?.child(reference)?;
        if r.attr("idx") == Some("0") {
            return None;
        }
        color_of(r.children.first()?, self.theme)
    }

    fn shape(&self, sp: &Node, tf: Transform, skip_ph: bool, out: &mut Vec<Item>) {
        let key = ph_of(sp);
        if skip_ph && key.is_some() {
            return;
        }
        let sp_pr = sp.child("spPr");
        let Some((rect, flip_h, flip_v)) = sp_pr
            .and_then(xfrm_of)
            .or_else(|| key.as_ref().and_then(|k| self.inherited_rect(k)))
        else {
            return;
        };
        let rect = tf.rect(rect);
        let empty = Node::default();
        let sp_pr = sp_pr.unwrap_or(&empty);
        let fill = match fill_of(sp_pr, self.theme) {
            Fill::Solid(c) => Some(c),
            Fill::Empty => None,
            Fill::Unset => self.style_color(sp, "fillRef"),
        };
        let line = match sp_pr.child("ln") {
            Some(ln) => {
                let width = num(ln, "w").unwrap_or(EMU_PT);
                match fill_of(ln, self.theme) {
                    Fill::Solid(c) => Some((c, width)),
                    Fill::Empty => None,
                    Fill::Unset => self.style_color(sp, "lnRef").map(|c| (c, width)),
                }
            }
            None => self.style_color(sp, "lnRef").map(|c| (c, EMU_PT)),
        };
        let prst = sp_pr.child("prstGeom").and_then(|g| g.attr("prst")).unwrap_or("rect");
        if sp.name == "cxnSp" || prst == "line" || prst.contains("Connector") {
            let (color, width) = line.unwrap_or((INK, EMU_PT));
            let (y1, y2) = if flip_v { (rect.max.y, rect.min.y) } else { (rect.min.y, rect.max.y) };
            let (x1, x2) = if flip_h { (rect.max.x, rect.min.x) } else { (rect.min.x, rect.max.x) };
            out.push(Item::Line {
                a: pos2(x1, y1),
                b: pos2(x2, y2),
                color,
                width,
            });
            return;
        }
        let shape_color = self.style_color(sp, "fontRef");
        let text = sp
            .child("txBody")
            .map(|b| self.text(b, key.as_ref(), shape_color, [91_440.0, 45_720.0, 91_440.0, 45_720.0]))
            .filter(|t| !t.is_blank());
        if fill.is_none() && line.is_none() && text.is_none() {
            return;
        }
        out.push(Item::Shape {
            rect,
            geom: match prst {
                "ellipse" => Geom::Ellipse,
                "roundRect" => Geom::Round,
                _ => Geom::Rect,
            },
            fill,
            line,
            text,
        });
    }

    fn picture(&self, pic: &Node, rels: &Rels, tf: Transform, skip_ph: bool, out: &mut Vec<Item>) {
        let key = ph_of(pic);
        if skip_ph && key.is_some() {
            return;
        }
        let Some(rel) = pic.find("blip").and_then(|b| b.attr("embed")) else {
            return;
        };
        let Some((name, _)) = rels.get(rel) else { return };
        let Some((rect, _, _)) = pic
            .child("spPr")
            .and_then(xfrm_of)
            .or_else(|| key.as_ref().and_then(|k| self.inherited_rect(k)))
        else {
            return;
        };
        let crop = pic
            .child("blipFill")
            .and_then(|b| b.child("srcRect"))
            .map_or([0.0; 4], |c| {
                ["l", "t", "r", "b"].map(|k| (num(c, k).unwrap_or(0.0) / 100_000.0).clamp(0.0, 0.9))
            });
        out.push(Item::Image {
            rect: tf.rect(rect),
            name: name.clone(),
            crop,
        });
    }

    fn frame(&self, frame: &Node, tf: Transform, out: &mut Vec<Item>) {
        let Some((rect, _, _)) = xfrm_of(frame) else { return };
        let rect = tf.rect(rect);
        let Some(table) = frame.find("tbl") else {
            // Charts, diagrams and embedded objects are not drawn.
            let label = Text {
                paras: vec![TextPara {
                    align: Align::Center,
                    bullet: false,
                    margin: 0.0,
                    size: 14.0,
                    runs: vec![Run {
                        text: "Objet non affiché (graphique, diagramme ou objet incorporé)".into(),
                        bold: false,
                        italic: true,
                        underline: false,
                        color: Color32::from_gray(0x70),
                        size: 14.0,
                    }],
                }],
                anchor: Anchor::Middle,
                insets: [91_440.0, 45_720.0, 91_440.0, 45_720.0],
                scale: 1.0,
                wrap: true,
            };
            out.push(Item::Shape {
                rect,
                geom: Geom::Rect,
                fill: Some(Color32::from_gray(0xF2)),
                line: Some((Color32::from_gray(0xBF), EMU_PT)),
                text: Some(label),
            });
            return;
        };
        let columns: Vec<f32> = table
            .child("tblGrid")
            .map(|g| g.children.iter().filter(|c| c.name == "gridCol").map(|c| num(c, "w").unwrap_or(1.0)).collect())
            .unwrap_or_default();
        let properties = table.child("tblPr");
        let styled = properties.is_some_and(|p| p.child("tableStyleId").is_some());
        let flag = |name: &str| properties.and_then(|p| p.attr(name)).is_some_and(|v| v == "1" || v == "true");
        let accent = self.theme.get("accent1").unwrap_or(Color32::from_rgb(0x44, 0x72, 0xC4));
        let mut rows = Vec::new();
        for (r, tr) in table.children.iter().filter(|c| c.name == "tr").enumerate() {
            let header = styled && flag("firstRow") && r == 0;
            let band = styled && flag("bandRow") && r > 0 && r % 2 == 1;
            let mut cells = Vec::new();
            for tc in tr.children.iter().filter(|c| c.name == "tc") {
                let pr = tc.child("tcPr");
                let hidden = tc.attr("hMerge") == Some("1") || tc.attr("vMerge") == Some("1");
                let explicit = pr.map(|p| fill_of(p, self.theme));
                let fill = match explicit {
                    Some(Fill::Solid(c)) => Some(c),
                    Some(Fill::Empty) => None,
                    _ if header => Some(accent),
                    _ if band => Some(lum(accent, 0.2, 0.8)),
                    _ => None,
                };
                let mut text = tc.child("txBody").map_or_else(
                    || Text {
                        paras: Vec::new(),
                        anchor: Anchor::Top,
                        insets: [91_440.0, 45_720.0, 91_440.0, 45_720.0],
                        scale: 1.0,
                        wrap: true,
                    },
                    |b| self.text(b, None, None, [91_440.0, 45_720.0, 91_440.0, 45_720.0]),
                );
                if header && fill == Some(accent) {
                    for run in text.paras.iter_mut().flat_map(|p| p.runs.iter_mut()) {
                        run.bold = true;
                        if run.color == self.theme.get("tx1").unwrap_or(INK) {
                            run.color = Color32::WHITE;
                        }
                    }
                }
                cells.push(TableCell {
                    fill,
                    text,
                    span: if hidden { 0 } else { tc.attr("gridSpan").and_then(|s| s.parse().ok()).unwrap_or(1) },
                });
            }
            rows.push(TableRow {
                height: num(tr, "h").unwrap_or(370_840.0) * tf.sy,
                cells,
            });
        }
        out.push(Item::Table {
            rect,
            columns: columns.iter().map(|w| w * tf.sx).collect(),
            rows,
        });
    }
}

fn background(root: &Node, theme: &Theme) -> Option<Color32> {
    let bg = root.find("bg")?;
    if let Some(props) = bg.child("bgPr")
        && let Fill::Solid(c) = fill_of(props, theme)
    {
        return Some(c);
    }
    color_of(bg.child("bgRef")?.children.first()?, theme)
}

fn count_words(items: &[Item]) -> usize {
    let words = |t: &Text| -> usize {
        t.paras
            .iter()
            .flat_map(|p| &p.runs)
            .map(|r| r.text.split_whitespace().count())
            .sum()
    };
    items
        .iter()
        .map(|i| match i {
            Item::Shape { text, .. } => text.as_ref().map_or(0, words),
            Item::Table { rows, .. } => rows.iter().flat_map(|r| &r.cells).map(|c| words(&c.text)).sum(),
            _ => 0,
        })
        .sum()
}

pub fn load(path: &Path) -> Result<Deck> {
    if fs::metadata(path)?.len() > MAX_FILE {
        bail!("Présentation trop volumineuse (limite : 128 Mo)");
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ppt" | "pps"))
    {
        bail!("Le format binaire .ppt n'est pas pris en charge : enregistrez la présentation au format .pptx");
    }
    let file = fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| {
        anyhow!("Ce fichier n'est pas une présentation PowerPoint valide (ou elle est protégée par un mot de passe)")
    })?;
    let main = read_part(&mut zip, "ppt/presentation.xml")?
        .ok_or_else(|| anyhow!("Ce fichier n'est pas une présentation : ppt/presentation.xml est absent"))?;
    let main = parse_xml(&main).context("ppt/presentation.xml")?;
    let rels = read_rels(&mut zip, "ppt/presentation.xml");
    let size = main
        .child("sldSz")
        .and_then(|s| Some(vec2(num(s, "cx")?, num(s, "cy")?)))
        .filter(|s| s.x > 0.0 && s.y > 0.0)
        .unwrap_or(vec2(9_144_000.0, 5_143_500.0));
    let theme = read_part(&mut zip, "ppt/theme/theme1.xml")?
        .and_then(|b| parse_xml(&b).ok())
        .map_or_else(|| Theme(HashMap::new()), |r| Theme::read(&r));

    let mut parts: HashMap<String, Node> = HashMap::new();
    let mut slides = Vec::new();
    let order = main.child("sldIdLst").map(|l| l.children.as_slice()).unwrap_or_default();
    for entry in order.iter().take(MAX_SLIDES) {
        // `id` is both the slide number and, with the `r:` prefix, the relationship.
        let Some((slide_path, _)) = entry
            .attrs
            .iter()
            .filter(|(k, _)| k == "id")
            .find_map(|(_, v)| rels.get(v))
        else {
            continue;
        };
        let slide_rels = read_rels(&mut zip, slide_path);
        let layout_path = slide_rels.values().find(|(_, t)| t.ends_with("/slideLayout")).map(|(p, _)| p.clone());
        let layout_rels = layout_path.as_deref().map(|p| read_rels(&mut zip, p)).unwrap_or_default();
        let master_path = layout_rels.values().find(|(_, t)| t.ends_with("/slideMaster")).map(|(p, _)| p.clone());
        let master_rels = master_path.as_deref().map(|p| read_rels(&mut zip, p)).unwrap_or_default();
        for p in [Some(slide_path.clone()), layout_path.clone(), master_path.clone()].into_iter().flatten() {
            if !parts.contains_key(&p) {
                let root = read_part(&mut zip, &p)?.and_then(|b| parse_xml(&b).ok()).unwrap_or_default();
                parts.insert(p, root);
            }
        }
        let empty = Node::default();
        let slide_root = &parts[slide_path];
        let layout_root = layout_path.as_ref().map_or(&empty, |p| &parts[p]);
        let master_root = master_path.as_ref().map_or(&empty, |p| &parts[p]);
        let parser = Parser {
            theme: &theme,
            layout: layout_root,
            master: master_root,
        };
        let mut items = Vec::new();
        let show_layout = slide_root.attr("showMasterSp") != Some("0");
        let show_master = show_layout && layout_root.attr("showMasterSp") != Some("0");
        if show_master && let Some(tree) = master_root.find("spTree") {
            parser.walk(tree, &master_rels, Transform::IDENTITY, true, 0, &mut items);
        }
        if show_layout && let Some(tree) = layout_root.find("spTree") {
            parser.walk(tree, &layout_rels, Transform::IDENTITY, true, 0, &mut items);
        }
        if let Some(tree) = slide_root.find("spTree") {
            parser.walk(tree, &slide_rels, Transform::IDENTITY, false, 0, &mut items);
        }
        let bg = [slide_root, layout_root, master_root]
            .into_iter()
            .find_map(|r| background(r, &theme))
            .unwrap_or(Color32::WHITE);
        slides.push(Slide { bg, items });
    }
    if slides.is_empty() {
        bail!("Cette présentation ne contient aucune diapositive");
    }
    let mut media = HashMap::new();
    let mut total = 0usize;
    'media: for slide in &slides {
        for item in &slide.items {
            if let Item::Image { name, .. } = item
                && !media.contains_key(name)
                && let Some(data) = read_part(&mut zip, name)?
            {
                total += data.len();
                if total > MAX_IMAGES_TOTAL {
                    break 'media;
                }
                media.insert(name.clone(), data);
            }
        }
    }
    let words = slides.iter().map(|s| count_words(&s.items)).sum();
    Ok(Deck {
        size,
        slides,
        media,
        words,
    })
}

// ---- Viewer -----------------------------------------------------------------------------

/// Draws the text of a shape inside `bounds`; `k` is the number of screen pixels per EMU.
fn paint_text(painter: &Painter, text: &Text, bounds: Rect, k: f32) {
    let inset = |i: usize| text.insets[i] * k;
    let inner = Rect::from_min_max(
        pos2(bounds.min.x + inset(0), bounds.min.y + inset(1)),
        pos2(bounds.max.x - inset(2), bounds.max.y - inset(3)),
    );
    let mut galleys = Vec::new();
    let mut total = 0.0;
    for para in &text.paras {
        let margin = para.margin * k;
        let mut job = LayoutJob {
            halign: para.align,
            ..LayoutJob::default()
        };
        job.wrap.max_width = if text.wrap { (inner.width() - margin).max(4.0) } else { f32::INFINITY };
        let px = |points: f32| points * text.scale * EMU_PT * k;
        let format = |run: &Run| {
            let size = px(run.size).clamp(1.0, 400.0);
            TextFormat {
                font_id: if run.bold { ui::semibold(size) } else { FontId::proportional(size) },
                color: run.color,
                italics: run.italic,
                underline: if run.underline { Stroke::new(1.0_f32, run.color) } else { Stroke::NONE },
                ..Default::default()
            }
        };
        if let (true, Some(first)) = (para.bullet, para.runs.first()) {
            job.append("•  ", 0.0, format(first));
        }
        for run in &para.runs {
            job.append(&run.text, 0.0, format(run));
        }
        if job.sections.is_empty() {
            let empty = Run {
                text: String::new(),
                bold: false,
                italic: false,
                underline: false,
                color: Color32::TRANSPARENT,
                size: para.size,
            };
            job.append(" ", 0.0, format(&empty));
        }
        let galley = painter.layout_job(job);
        total += galley.size().y;
        galleys.push((galley, para.align, margin));
    }
    let mut y = match text.anchor {
        Anchor::Top => inner.top(),
        Anchor::Middle => inner.center().y - total / 2.0,
        Anchor::Bottom => inner.bottom() - total,
    };
    for (galley, align, margin) in galleys {
        let left = inner.left() + margin;
        let width = (inner.width() - margin).max(0.0);
        let x = match align {
            Align::Min => left,
            Align::Center => left + width / 2.0,
            Align::Max => left + width,
        };
        let height = galley.size().y;
        painter.galley(pos2(x, y), galley, Color32::BLACK);
        y += height;
    }
}

fn paint_slide(ui: &egui::Ui, painter: &Painter, rect: Rect, deck: &Deck, slide: &Slide, textures: &mut Textures) {
    let k = rect.width() / deck.size.x;
    let screen = |r: Rect| Rect::from_min_size(rect.min + r.min.to_vec2() * k, r.size() * k);
    painter.rect_filled(rect, 0.0, slide.bg);
    for item in &slide.items {
        match item {
            Item::Shape { rect: r, geom, fill, line, text } => {
                let r = screen(*r);
                let stroke = line.map_or(Stroke::NONE, |(c, w)| Stroke::new((w * k).max(0.5), c));
                match geom {
                    Geom::Rect => {
                        painter.rect(r, 0.0, fill.unwrap_or(Color32::TRANSPARENT), stroke, StrokeKind::Middle);
                    }
                    Geom::Round => {
                        let radius = (r.width().min(r.height()) * 0.166_67).round().clamp(0.0, 255.0) as u8;
                        painter.rect(
                            r,
                            CornerRadius::same(radius),
                            fill.unwrap_or(Color32::TRANSPARENT),
                            stroke,
                            StrokeKind::Middle,
                        );
                    }
                    Geom::Ellipse => {
                        let radius = r.size() / 2.0;
                        if let Some(fill) = fill {
                            painter.add(Shape::ellipse_filled(r.center(), radius, *fill));
                        }
                        if line.is_some() {
                            painter.add(Shape::ellipse_stroke(r.center(), radius, stroke));
                        }
                    }
                }
                if let Some(text) = text {
                    paint_text(painter, text, r, k);
                }
            }
            Item::Line { a, b, color, width } => {
                let to = |p: &Pos2| rect.min + p.to_vec2() * k;
                painter.line_segment([to(a), to(b)], Stroke::new((width * k).max(0.5), *color));
            }
            Item::Image { rect: r, name, crop } => {
                let entry = textures
                    .entry(name.clone())
                    .or_insert_with(|| deck.media.get(name).and_then(|d| decode_image(ui.ctx(), name, d)));
                let r = screen(*r);
                match entry {
                    Some((texture, _)) => {
                        let uv = Rect::from_min_max(pos2(crop[0], crop[1]), pos2(1.0 - crop[2], 1.0 - crop[3]));
                        painter.image(texture.id(), r, uv, Color32::WHITE);
                    }
                    None => {
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_gray(0xBF)), StrokeKind::Inside);
                    }
                }
            }
            Item::Table { rect: r, columns, rows } => {
                let r = screen(*r);
                let mut y = r.top();
                for row in rows {
                    let height = row.height * k;
                    let mut x = r.left();
                    let mut column = 0;
                    for cell in &row.cells {
                        if cell.span == 0 {
                            x += columns.get(column).copied().unwrap_or(0.0) * k;
                            column += 1;
                            continue;
                        }
                        let width: f32 = (column..column + cell.span).map(|c| columns.get(c).copied().unwrap_or(0.0)).sum::<f32>() * k;
                        let cell_rect = Rect::from_min_size(pos2(x, y), vec2(width, height));
                        if let Some(fill) = cell.fill {
                            painter.rect_filled(cell_rect, 0.0, fill);
                        }
                        painter.rect_stroke(cell_rect, 0.0, Stroke::new(1.0_f32, Color32::from_gray(0xA0)), StrokeKind::Inside);
                        paint_text(&painter.with_clip_rect(cell_rect.intersect(painter.clip_rect())), &cell.text, cell_rect, k);
                        x += width;
                        column += cell.span;
                    }
                    y += height;
                }
            }
        }
    }
}

pub fn show(ui: &mut egui::Ui, deck: &Deck, textures: &mut Textures, path: &Path, zoom: f32) {
    let t = ui::theme(ui.ctx());
    egui::ScrollArea::vertical()
        .id_salt(("pptx", path))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                let width = (ui.available_width() - 32.0).clamp(240.0, 960.0 * zoom);
                let height = width * deck.size.y / deck.size.x;
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(12.0);
                for (i, slide) in deck.slides.iter().enumerate() {
                    let (area, _) = ui.allocate_exact_size(vec2(width, height + 34.0), egui::Sense::hover());
                    if !ui.is_rect_visible(area) {
                        continue;
                    }
                    let painter = ui.painter_at(area);
                    painter.text(
                        area.left_top() + vec2(0.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("Diapositive {}", i + 1),
                        FontId::proportional(12.0),
                        t.text_secondary,
                    );
                    let rect = Rect::from_min_size(area.left_top() + vec2(0.0, 20.0), vec2(width, height));
                    paint_slide(ui, &ui.painter_at(rect), rect, deck, slide, textures);
                    painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, t.border), StrokeKind::Outside);
                }
            });
        });
}

// ---- PDF --------------------------------------------------------------------------------

fn pdf_text(pdf: &mut Pdf, text: &Text, bounds: Rect) {
    let pt = 1.0 / EMU_PT;
    let inner = Rect::from_min_max(
        pos2(bounds.min.x + text.insets[0] * pt, bounds.min.y + text.insets[1] * pt),
        pos2(bounds.max.x - text.insets[2] * pt, bounds.max.y - text.insets[3] * pt),
    );
    let mut blocks = Vec::new();
    let mut total = 0.0;
    for para in &text.paras {
        let margin = para.margin * pt;
        let mut items = Vec::new();
        let span = |run: &Run, content: String| Span {
            text: content,
            font: Font::of(run.bold, run.italic),
            size: run.size * text.scale,
            color: run.color,
            underline: run.underline,
            strike: false,
            bg: None,
            raise: 0.0,
        };
        if let (true, Some(first)) = (para.bullet, para.runs.first()) {
            items.push(Inl::Text(span(first, "•  ".into())));
        }
        for run in &para.runs {
            items.push(Inl::Text(span(run, run.text.clone())));
        }
        let width = if text.wrap { (inner.width() - margin).max(8.0) } else { 10_000.0 };
        let lines = flow(&items, width, para.size * text.scale);
        total += lines.iter().map(|l| l.height).sum::<f32>();
        blocks.push((lines, para.align, margin));
    }
    let mut y = match text.anchor {
        Anchor::Top => inner.top(),
        Anchor::Middle => inner.center().y - total / 2.0,
        Anchor::Bottom => inner.bottom() - total,
    };
    for (lines, align, margin) in blocks {
        for line in &lines {
            pdf.draw_line(line, inner.left() + margin, y, (inner.width() - margin).max(0.0), align);
            y += line.height;
        }
    }
}

pub fn export_pdf(deck: &Deck, target: &Path) -> Result<()> {
    let pt = 1.0 / EMU_PT;
    let mut pdf = Pdf::default();
    let mut images: HashMap<String, Option<usize>> = HashMap::new();
    for slide in &deck.slides {
        pdf.page(deck.size.x * pt, deck.size.y * pt);
        pdf.fill_rect(0.0, 0.0, deck.size.x * pt, deck.size.y * pt, slide.bg);
        let scale = |r: Rect| Rect::from_min_size(pos2(r.min.x * pt, r.min.y * pt), r.size() * pt);
        for item in &slide.items {
            match item {
                Item::Shape { rect, geom, fill, line, text } => {
                    let r = scale(*rect);
                    let stroke = line.map(|(c, w)| (c, (w * pt).max(0.25)));
                    match geom {
                        Geom::Ellipse => pdf.ellipse(r.min.x, r.min.y, r.width(), r.height(), *fill, stroke),
                        Geom::Round => {
                            let radius = r.width().min(r.height()) * 0.166_67;
                            pdf.rounded_rect(r.min.x, r.min.y, r.width(), r.height(), radius, *fill, stroke);
                        }
                        Geom::Rect => pdf.rounded_rect(r.min.x, r.min.y, r.width(), r.height(), 0.0, *fill, stroke),
                    }
                    if let Some(text) = text {
                        pdf_text(&mut pdf, text, r);
                    }
                }
                Item::Line { a, b, color, width } => {
                    pdf.line(a.x * pt, a.y * pt, b.x * pt, b.y * pt, *color, (width * pt).max(0.25));
                }
                Item::Image { rect, name, crop } => {
                    let r = scale(*rect);
                    let index = *images.entry(name.clone()).or_insert_with(|| {
                        let img = deck.media.get(name).and_then(|d| image::load_from_memory(d).ok())?;
                        Some(pdf.add_image(&img))
                    });
                    match index {
                        Some(index) => {
                            let visible_w = 1.0 - crop[0] - crop[2];
                            let visible_h = 1.0 - crop[1] - crop[3];
                            let (full_w, full_h) = (r.width() / visible_w.max(0.05), r.height() / visible_h.max(0.05));
                            pdf.push_clip(r.min.x, r.min.y, r.width(), r.height());
                            pdf.image(r.min.x - crop[0] * full_w, r.min.y - crop[1] * full_h, full_w, full_h, index);
                            pdf.pop_clip();
                        }
                        None => pdf.stroke_rect(r.min.x, r.min.y, r.width(), r.height(), Color32::from_gray(0xBF), 0.5),
                    }
                }
                Item::Table { rect, columns, rows } => {
                    let r = scale(*rect);
                    let mut y = r.min.y;
                    for row in rows {
                        let height = row.height * pt;
                        let mut x = r.min.x;
                        let mut column = 0;
                        for cell in &row.cells {
                            if cell.span == 0 {
                                x += columns.get(column).copied().unwrap_or(0.0) * pt;
                                column += 1;
                                continue;
                            }
                            let width: f32 =
                                (column..column + cell.span).map(|c| columns.get(c).copied().unwrap_or(0.0)).sum::<f32>() * pt;
                            if let Some(fill) = cell.fill {
                                pdf.fill_rect(x, y, width, height, fill);
                            }
                            pdf.stroke_rect(x, y, width, height, Color32::from_gray(0xA0), 0.5);
                            pdf.push_clip(x, y, width, height);
                            pdf_text(&mut pdf, &cell.text, Rect::from_min_size(pos2(x, y), vec2(width, height)));
                            pdf.pop_clip();
                            x += width;
                            column += cell.span;
                        }
                        y += height;
                    }
                }
            }
        }
    }
    fs::write(target, pdf.finish())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PRESENTATION: &str = r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#;

    const PRESENTATION_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;

    const SLIDE_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;

    const LAYOUT_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;

    const LAYOUT: &str = r#"<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="457200" y="228600"/><a:ext cx="8229600" cy="857250"/></a:xfrm></p:spPr></p:sp>
</p:spTree></p:cSld></p:sldLayout>"#;

    const MASTER: &str = r#"<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="EEEEEE"/></a:solidFill></p:bgPr></p:bg><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Barre"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="100000"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:spPr></p:sp>
</p:spTree></p:cSld>
<p:txStyles><p:titleStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="4000"/></a:lvl1pPr></p:titleStyle></p:txStyles></p:sldMaster>"#;

    const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="t">
<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="1F497D"/></a:dk2><a:lt2><a:srgbClr val="EEECE1"/></a:lt2>
<a:accent1><a:srgbClr val="4F81BD"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"#;

    const SLIDE: &str = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Titre"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/>
 <p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="fr-FR" b="1"/><a:t>Bilan annuel</a:t></a:r></a:p></p:txBody></p:sp>
<p:grpSp><p:nvGrpSpPr><p:cNvPr id="9" name="Groupe"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
 <p:grpSpPr><a:xfrm><a:off x="1000000" y="1000000"/><a:ext cx="2000000" cy="1000000"/><a:chOff x="0" y="0"/><a:chExt cx="1000000" cy="500000"/></a:xfrm></p:grpSpPr>
 <p:sp><p:nvSpPr><p:cNvPr id="4" name="Carré"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="100000" y="100000"/><a:ext cx="400000" cy="200000"/></a:xfrm><a:prstGeom prst="ellipse"/>
  <a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:ln w="25400"><a:solidFill><a:srgbClr val="0000FF"/></a:solidFill></a:ln></p:spPr>
  <p:txBody><a:bodyPr anchor="ctr"/><a:p><a:pPr algn="ctr"/><a:r><a:rPr sz="2000"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr><a:t>Rond</a:t></a:r></a:p></p:txBody></p:sp>
</p:grpSp>
<p:pic><p:nvPicPr><p:cNvPr id="5" name="Image"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
 <p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="10000" r="10000"/></p:blipFill>
 <p:spPr><a:xfrm><a:off x="5000000" y="2000000"/><a:ext cx="1000000" cy="500000"/></a:xfrm></p:spPr></p:pic>
<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="6" name="Tableau"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
 <p:xfrm><a:off x="500000" y="3000000"/><a:ext cx="4000000" cy="600000"/></p:xfrm>
 <a:graphic><a:graphicData><a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{X}</a:tableStyleId></a:tblPr>
  <a:tblGrid><a:gridCol w="2000000"/><a:gridCol w="2000000"/></a:tblGrid>
  <a:tr h="300000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Mois</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Total</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr>
  <a:tr h="300000"><a:tc gridSpan="2"><a:txBody><a:bodyPr/><a:p><a:r><a:t>fusion</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc><a:tc hMerge="1"><a:txBody><a:bodyPr/><a:p/></a:txBody><a:tcPr/></a:tc></a:tr>
 </a:tbl></a:graphicData></a:graphic></p:graphicFrame>
</p:spTree></p:cSld></p:sld>"#;

    fn png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn sample(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("atelier-pptx-{}-{name}.pptx", std::process::id()));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("ppt/presentation.xml", PRESENTATION.as_bytes().to_vec()),
            ("ppt/_rels/presentation.xml.rels", PRESENTATION_RELS.as_bytes().to_vec()),
            ("ppt/slides/slide1.xml", SLIDE.as_bytes().to_vec()),
            ("ppt/slides/_rels/slide1.xml.rels", SLIDE_RELS.as_bytes().to_vec()),
            ("ppt/slideLayouts/slideLayout1.xml", LAYOUT.as_bytes().to_vec()),
            ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", LAYOUT_RELS.as_bytes().to_vec()),
            ("ppt/slideMasters/slideMaster1.xml", MASTER.as_bytes().to_vec()),
            ("ppt/theme/theme1.xml", THEME.as_bytes().to_vec()),
            ("ppt/media/image1.png", png()),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(&data).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn reads_shapes_inheritance_groups_pictures_and_tables() {
        let path = sample("read");
        let deck = load(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(deck.size, vec2(9_144_000.0, 5_143_500.0));
        assert_eq!(deck.slides.len(), 1);
        let slide = &deck.slides[0];
        assert_eq!(slide.bg, Color32::from_rgb(0xEE, 0xEE, 0xEE), "background of the master");
        // The bar of the master, the title, the group's ellipse, the picture, the table.
        assert_eq!(slide.items.len(), 5);
        let Item::Shape { fill, .. } = &slide.items[0] else { panic!("master bar") };
        assert_eq!(*fill, Some(Color32::from_rgb(0x4F, 0x81, 0xBD)));

        let Item::Shape { rect, text, .. } = &slide.items[1] else { panic!("title") };
        assert_eq!(rect.min, pos2(457_200.0, 228_600.0), "position of the layout");
        let text = text.as_ref().unwrap();
        assert_eq!(text.paras[0].runs[0].text, "Bilan annuel");
        assert_eq!(text.paras[0].runs[0].size, 40.0, "size of the master");
        assert_eq!(text.paras[0].align, Align::Center);
        assert_eq!(text.anchor, Anchor::Middle);
        assert!(text.paras[0].runs[0].bold);

        let Item::Shape { rect, geom, fill, line, text } = &slide.items[2] else { panic!("ellipse") };
        assert_eq!(*geom, Geom::Ellipse);
        // The group is drawn twice as large as its content.
        assert_eq!(rect.min, pos2(1_200_000.0, 1_200_000.0));
        assert_eq!(rect.size(), vec2(800_000.0, 400_000.0));
        assert_eq!(*fill, Some(Color32::RED));
        assert_eq!(*line, Some((Color32::BLUE, 25_400.0)));
        assert_eq!(text.as_ref().unwrap().paras[0].runs[0].color, Color32::WHITE);

        let Item::Image { name, crop, .. } = &slide.items[3] else { panic!("picture") };
        assert_eq!(name, "ppt/media/image1.png");
        assert_eq!(*crop, [0.1, 0.0, 0.1, 0.0]);
        assert!(deck.media.contains_key(name));

        let Item::Table { columns, rows, .. } = &slide.items[4] else { panic!("table") };
        assert_eq!(columns, &[2_000_000.0, 2_000_000.0]);
        assert_eq!(rows[0].cells[0].fill, Some(Color32::from_rgb(0x4F, 0x81, 0xBD)), "header of the table style");
        assert_eq!(rows[0].cells[0].text.paras[0].runs[0].color, Color32::WHITE);
        assert_eq!(rows[1].cells[0].span, 2);
        assert_eq!(rows[1].cells[1].span, 0);
        assert!(deck.words >= 6);
    }

    #[test]
    fn exports_and_renders_slides() {
        let path = sample("export");
        let deck = load(&path).unwrap();
        let _ = fs::remove_file(&path);
        let target = std::env::temp_dir().join(format!("atelier-pptx-{}.pdf", std::process::id()));
        export_pdf(&deck, &target).unwrap();
        let bytes = fs::read(&target).unwrap();
        let _ = fs::remove_file(&target);
        assert!(bytes.starts_with(b"%PDF-"));

        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut textures = Textures::new();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, &deck, &mut textures, Path::new("a.pptx"), 1.0));
            });
        }
        assert!(textures.values().all(Option::is_some));
    }

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("ppt/slides/slide1.xml", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt/presentation.xml", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides/slide1.xml", "/ppt/media/b.png"), "ppt/media/b.png");
    }

    #[test]
    fn rejects_other_files() {
        let text = std::env::temp_dir().join(format!("atelier-pptx-{}-text.pptx", std::process::id()));
        fs::write(&text, "not a zip").unwrap();
        let error = format!("{:#}", load(&text).err().unwrap());
        let _ = fs::remove_file(&text);
        assert!(error.contains("pas une présentation"), "{error}");
    }
}
