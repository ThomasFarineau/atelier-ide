//! Minimal PDF writer for the Office exports: Helvetica text with word wrapping, rectangles,
//! ellipses, lines, clipping and images, on pages measured in points. The origin of every
//! coordinate is the top left corner of the page, `y` growing downwards.

use eframe::egui::{Align, Color32};
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Font {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Font {
    pub fn of(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Self::Regular,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }
    fn index(self) -> usize {
        self as usize
    }
    fn base_name(self) -> &'static str {
        ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique"][self.index()]
    }
    fn bold(self) -> bool {
        matches!(self, Self::Bold | Self::BoldItalic)
    }
}

/// Advance widths of Helvetica for the characters 32 to 126, in 1/1000 em.
const ASCII_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space - /
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0-9
    278, 278, 584, 584, 584, 556, 1015, // : ; < = > ? @
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // A-M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // N-Z
    278, 278, 278, 469, 556, 333, // [ \ ] ^ _ `
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // a-m
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // n-z
    334, 260, 334, 584, // { | } ~
];

/// The letter whose width stands for an accented one.
fn fold(c: char) -> Option<char> {
    Some(match c {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'œ' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'À'..='Å' => 'A',
        'Ç' => 'C',
        'È'..='Ë' => 'E',
        'Ì'..='Ï' => 'I',
        'Ñ' => 'N',
        'Ò'..='Ö' | 'Œ' => 'O',
        'Ù'..='Ü' => 'U',
        'Ý' => 'Y',
        _ => return None,
    })
}

fn glyph_width(c: char) -> f32 {
    f32::from(match c {
        ' '..='~' => ASCII_WIDTHS[(c as u32 - 32) as usize],
        '…' | '—' => 1000,
        '‘' | '’' => 222,
        '“' | '”' => 333,
        '•' | '◦' | '▪' => 350,
        '\u{a0}' | '\t' => 278,
        c => fold(c).map_or(556, |base| ASCII_WIDTHS[(base as u32 - 32) as usize]),
    })
}

/// The byte of a character in the WinAnsi encoding, `None` for control characters.
fn encode(c: char) -> Option<u8> {
    Some(match c {
        '\t' => b' ',
        '\u{2011}' => b'-',
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '…' => 0x85,
        'Œ' => 0x8C,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' | '▪' => 0x95,
        '◦' => 0xB0,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        'œ' => 0x9C,
        c if c.is_control() => return None,
        _ => b'?',
    })
}

pub fn text_width(font: Font, size: f32, text: &str) -> f32 {
    let em: f32 = text.chars().filter(|c| encode(*c).is_some()).map(glyph_width).sum();
    em * size / 1000.0 * if font.bold() { 1.05 } else { 1.0 }
}

/// The longest start of `text` that fits in `width`, ending with an ellipsis when cut.
pub fn fit(font: Font, size: f32, text: &str, width: f32) -> String {
    if text_width(font, size, text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = text_width(font, size, "…");
    for c in text.chars() {
        let w = text_width(font, size, c.encode_utf8(&mut [0; 4]));
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('…');
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

fn num(v: f32) -> String {
    let text = format!("{v:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn rgb(c: Color32) -> String {
    let channel = |v: u8| num(f32::from(v) / 255.0);
    format!("{} {} {}", channel(c.r()), channel(c.g()), channel(c.b()))
}

struct Img {
    w: u32,
    h: u32,
    rgb: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

struct Page {
    w: f32,
    h: f32,
    content: Vec<u8>,
    images: Vec<usize>,
}

#[derive(Default)]
pub struct Pdf {
    pages: Vec<Page>,
    images: Vec<Img>,
}

/// Largest side of an embedded image, in pixels.
const MAX_IMAGE_SIDE: u32 = 1600;
const KAPPA: f32 = 0.552_284_8;

impl Pdf {
    pub fn page(&mut self, w: f32, h: f32) {
        self.pages.push(Page {
            w,
            h,
            content: Vec::new(),
            images: Vec::new(),
        });
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn cur(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a page was started")
    }

    fn op(&mut self, text: String) {
        let page = self.cur();
        page.content.extend_from_slice(text.as_bytes());
        page.content.push(b'\n');
    }

    /// Text with its baseline at `y`.
    pub fn text(&mut self, x: f32, y: f32, font: Font, size: f32, color: Color32, text: &str) {
        if text.is_empty() {
            return;
        }
        let page_h = self.cur().h;
        let mut bytes = Vec::with_capacity(text.len() + 32);
        bytes.extend_from_slice(
            format!("BT /F{} {} Tf {} rg {} {} Td (", font.index(), num(size), rgb(color), num(x), num(page_h - y))
                .as_bytes(),
        );
        for byte in text.chars().filter_map(encode) {
            if matches!(byte, b'(' | b')' | b'\\') {
                bytes.push(b'\\');
            }
            bytes.push(byte);
        }
        bytes.extend_from_slice(b") Tj ET\n");
        self.cur().content.extend_from_slice(&bytes);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color32) {
        let page_h = self.cur().h;
        self.op(format!("{} rg {} {} {} {} re f", rgb(color), num(x), num(page_h - y - h), num(w), num(h)));
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color32, width: f32) {
        let page_h = self.cur().h;
        self.op(format!(
            "{} RG {} w {} {} {} {} re S",
            rgb(color),
            num(width),
            num(x),
            num(page_h - y - h),
            num(w),
            num(h)
        ));
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color32, width: f32) {
        let page_h = self.cur().h;
        self.op(format!(
            "{} RG {} w {} {} m {} {} l S",
            rgb(color),
            num(width.max(0.1)),
            num(x1),
            num(page_h - y1),
            num(x2),
            num(page_h - y2)
        ));
    }

    /// Paints the current path: filled, stroked or both.
    fn paint(&mut self, fill: Option<Color32>, stroke: Option<(Color32, f32)>) {
        let mut ops = String::new();
        if let Some(color) = fill {
            ops += &format!("{} rg ", rgb(color));
        }
        if let Some((color, width)) = stroke {
            ops += &format!("{} RG {} w ", rgb(color), num(width.max(0.1)));
        }
        ops += match (fill.is_some(), stroke.is_some()) {
            (true, true) => "B",
            (true, false) => "f",
            (false, true) => "S",
            (false, false) => "n",
        };
        self.op(ops);
    }

    pub fn ellipse(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Option<Color32>, stroke: Option<(Color32, f32)>) {
        let page_h = self.cur().h;
        let (rx, ry) = (w / 2.0, h / 2.0);
        let (cx, cy) = (x + rx, page_h - y - ry);
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.op(format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h",
            num(cx + rx),
            num(cy),
            num(cx + rx),
            num(cy + ky),
            num(cx + kx),
            num(cy + ry),
            num(cx),
            num(cy + ry),
            num(cx - kx),
            num(cy + ry),
            num(cx - rx),
            num(cy + ky),
            num(cx - rx),
            num(cy),
            num(cx - rx),
            num(cy - ky),
            num(cx - kx),
            num(cy - ry),
            num(cx),
            num(cy - ry),
            num(cx + kx),
            num(cy - ry),
            num(cx + rx),
            num(cy - ky),
            num(cx + rx),
            num(cy),
        ));
        self.paint(fill, stroke);
    }

    pub fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        fill: Option<Color32>,
        stroke: Option<(Color32, f32)>,
    ) {
        let page_h = self.cur().h;
        let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let k = r * (1.0 - KAPPA);
        let (left, right, bottom, top) = (x, x + w, page_h - y - h, page_h - y);
        let p = |v: f32| num(v);
        self.op(format!(
            "{} {} m {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c h",
            p(left + r), p(bottom),
            p(right - r), p(bottom),
            p(right - k), p(bottom), p(right), p(bottom + k), p(right), p(bottom + r),
            p(right), p(top - r),
            p(right), p(top - k), p(right - k), p(top), p(right - r), p(top),
            p(left + r), p(top),
            p(left + k), p(top), p(left), p(top - k), p(left), p(top - r),
            p(left), p(bottom + r),
            p(left), p(bottom + k), p(left + k), p(bottom), p(left + r), p(bottom),
        ));
        self.paint(fill, stroke);
    }

    /// Everything drawn until [`Pdf::pop_clip`] is cut to this rectangle.
    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let page_h = self.cur().h;
        self.op(format!("q {} {} {} {} re W n", num(x), num(page_h - y - h), num(w), num(h)));
    }

    pub fn pop_clip(&mut self) {
        self.op("Q".into());
    }

    /// Embeds a picture, to place with [`Pdf::image`]. Transparent pixels stay transparent.
    pub fn add_image(&mut self, image: &image::DynamicImage) -> usize {
        let image = if image.width().max(image.height()) > MAX_IMAGE_SIDE {
            image.thumbnail(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE)
        } else {
            image.clone()
        };
        let rgba = image.to_rgba8();
        let mut color = Vec::with_capacity(rgba.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(rgba.len() / 4);
        for pixel in rgba.pixels() {
            color.extend_from_slice(&pixel.0[..3]);
            alpha.push(pixel.0[3]);
        }
        let transparent = alpha.iter().any(|a| *a != 255);
        self.images.push(Img {
            w: rgba.width(),
            h: rgba.height(),
            rgb: deflate(&color),
            alpha: transparent.then(|| deflate(&alpha)),
        });
        self.images.len() - 1
    }

    pub fn image(&mut self, x: f32, y: f32, w: f32, h: f32, index: usize) {
        let page_h = self.cur().h;
        let page = self.cur();
        if !page.images.contains(&index) {
            page.images.push(index);
        }
        self.op(format!("q {} 0 0 {} {} {} cm /Im{index} Do Q", num(w), num(h), num(x), num(page_h - y - h)));
    }

    /// Draws one line of [`flow`] with its top left corner at `(x, y)`, aligned in `avail`.
    pub fn draw_line(&mut self, line: &Line, x: f32, y: f32, avail: f32, align: Align) {
        let mut cx = match align {
            Align::Min => x,
            Align::Center => x + ((avail - line.width) / 2.0).max(0.0),
            Align::Max => x + (avail - line.width).max(0.0),
        };
        let base = y + line.ascent;
        for piece in &line.pieces {
            match piece {
                Piece::Text(span, w) => {
                    if let Some(bg) = span.bg {
                        self.fill_rect(cx, base - span.size * 0.93, *w, span.size * 1.15, bg);
                    }
                    self.text(cx, base - span.raise, span.font, span.size, span.color, &span.text);
                    if span.underline {
                        self.line(cx, base + span.size * 0.12, cx + w, base + span.size * 0.12, span.color, span.size * 0.05);
                    }
                    if span.strike {
                        self.line(cx, base - span.size * 0.3, cx + w, base - span.size * 0.3, span.color, span.size * 0.05);
                    }
                    cx += w;
                }
                Piece::Image { index, w, h } => {
                    self.image(cx, base - h, *w, *h, *index);
                    cx += w;
                }
            }
        }
    }

    pub fn finish(self) -> Vec<u8> {
        // Object numbers: catalog, page tree, four fonts, then images and pages.
        let mut next = 7usize;
        let image_ids: Vec<(usize, Option<usize>)> = self
            .images
            .iter()
            .map(|img| {
                let id = next;
                next += 1;
                let mask = img.alpha.is_some().then(|| {
                    next += 1;
                    id + 1
                });
                (id, mask)
            })
            .collect();
        let page_ids: Vec<(usize, usize)> = self
            .pages
            .iter()
            .map(|_| {
                next += 2;
                (next - 2, next - 1)
            })
            .collect();
        let total = next - 1;

        let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0usize; total + 1];
        let mut write = |out: &mut Vec<u8>, id: usize, head: &str, stream: Option<&[u8]>| {
            offsets[id] = out.len();
            out.extend_from_slice(format!("{id} 0 obj\n{head}").as_bytes());
            if let Some(data) = stream {
                out.extend_from_slice(b"\nstream\n");
                out.extend_from_slice(data);
                out.extend_from_slice(b"\nendstream");
            }
            out.extend_from_slice(b"\nendobj\n");
        };

        write(&mut out, 1, "<< /Type /Catalog /Pages 2 0 R >>", None);
        let kids: Vec<String> = page_ids.iter().map(|(id, _)| format!("{id} 0 R")).collect();
        write(
            &mut out,
            2,
            &format!("<< /Type /Pages /Count {} /Kids [{}] >>", page_ids.len(), kids.join(" ")),
            None,
        );
        for (i, id) in (3..=6).enumerate() {
            let font = [Font::Regular, Font::Bold, Font::Italic, Font::BoldItalic][i];
            write(
                &mut out,
                id,
                &format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                    font.base_name()
                ),
                None,
            );
        }
        for (img, (id, mask)) in self.images.iter().zip(&image_ids) {
            let smask = mask.map_or(String::new(), |m| format!(" /SMask {m} 0 R"));
            write(
                &mut out,
                *id,
                &format!(
                    "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {}{smask} >>",
                    img.w,
                    img.h,
                    img.rgb.len()
                ),
                Some(&img.rgb),
            );
            if let (Some(alpha), Some(m)) = (&img.alpha, mask) {
                write(
                    &mut out,
                    *m,
                    &format!(
                        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                        img.w,
                        img.h,
                        alpha.len()
                    ),
                    Some(alpha),
                );
            }
        }
        for (page, (page_id, content_id)) in self.pages.iter().zip(&page_ids) {
            let xobjects: String = page
                .images
                .iter()
                .map(|i| format!("/Im{i} {} 0 R ", image_ids[*i].0))
                .collect();
            write(
                &mut out,
                *page_id,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {content_id} 0 R /Resources << /Font << /F0 3 0 R /F1 4 0 R /F2 5 0 R /F3 6 0 R >> /XObject << {xobjects}>> >> >>",
                    num(page.w),
                    num(page.h)
                ),
                None,
            );
            let data = deflate(&page.content);
            write(
                &mut out,
                *content_id,
                &format!("<< /Filter /FlateDecode /Length {} >>", data.len()),
                Some(&data),
            );
        }

        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", total + 1).as_bytes());
        for offset in &offsets[1..] {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", total + 1).as_bytes(),
        );
        out
    }
}

// ---- Text flow --------------------------------------------------------------------------

/// A run of text with one style.
#[derive(Clone)]
pub struct Span {
    pub text: String,
    pub font: Font,
    pub size: f32,
    pub color: Color32,
    pub underline: bool,
    pub strike: bool,
    pub bg: Option<Color32>,
    /// Baseline shift, positive upwards (superscripts).
    pub raise: f32,
}

/// Input of [`flow`]: styled text or a picture placed in the line.
pub enum Inl {
    Text(Span),
    Image { index: usize, w: f32, h: f32 },
}

pub enum Piece {
    Text(Span, f32),
    Image { index: usize, w: f32, h: f32 },
}

pub struct Line {
    pub pieces: Vec<Piece>,
    pub width: f32,
    pub height: f32,
    /// Distance from the top of the line to its baseline.
    pub ascent: f32,
}

#[derive(Default)]
struct Builder {
    pieces: Vec<Piece>,
    width: f32,
    trailing: f32,
    size: f32,
    image: f32,
    content: bool,
    wrapped: bool,
}

impl Builder {
    fn take(&mut self, default_size: f32) -> Line {
        let size = if self.size > 0.0 { self.size } else { default_size };
        let ascent = (size * 0.93).max(self.image);
        let line = Line {
            pieces: std::mem::take(&mut self.pieces),
            width: (self.width - self.trailing).max(0.0),
            height: ascent + size * 0.27,
            ascent,
        };
        *self = Builder::default();
        line
    }
}

/// Breaks styled text into lines of at most `width` points, cutting between words (and inside
/// a word only when it is wider than the line). Never returns an empty list.
pub fn flow(items: &[Inl], width: f32, default_size: f32) -> Vec<Line> {
    let width = width.max(8.0);
    let mut lines = Vec::new();
    let mut b = Builder::default();
    for item in items {
        match item {
            Inl::Image { index, w, h } => {
                if b.content && b.width + w > width {
                    lines.push(b.take(default_size));
                    b.wrapped = true;
                }
                b.pieces.push(Piece::Image {
                    index: *index,
                    w: *w,
                    h: *h,
                });
                b.width += w;
                b.trailing = 0.0;
                b.image = b.image.max(*h);
                b.content = true;
            }
            Inl::Text(span) => {
                let measure = |s: &str| text_width(span.font, span.size, s);
                let mut rest = span.text.as_str();
                while !rest.is_empty() {
                    let first = rest.chars().next().expect("not empty");
                    if first == '\n' {
                        b.size = b.size.max(span.size);
                        lines.push(b.take(default_size));
                        rest = &rest[1..];
                        continue;
                    }
                    let space = first.is_whitespace();
                    let end = rest
                        .char_indices()
                        .find(|(_, c)| *c == '\n' || c.is_whitespace() != space)
                        .map_or(rest.len(), |(i, _)| i);
                    let (token, tail) = rest.split_at(end);
                    rest = tail;
                    let w = measure(token);
                    let piece = |text: &str, w: f32| {
                        let mut s = span.clone();
                        s.text = text.to_string();
                        Piece::Text(s, w)
                    };
                    if space {
                        if b.width + w > width {
                            lines.push(b.take(default_size));
                            b.wrapped = true;
                        } else if !(b.wrapped && !b.content) {
                            b.pieces.push(piece(token, w));
                            b.width += w;
                            b.trailing += w;
                            b.size = b.size.max(span.size);
                        }
                        continue;
                    }
                    if b.content && b.width + w > width {
                        lines.push(b.take(default_size));
                        b.wrapped = true;
                    }
                    if w <= width {
                        b.pieces.push(piece(token, w));
                        b.width += w;
                        b.trailing = 0.0;
                        b.size = b.size.max(span.size);
                        b.content = true;
                        continue;
                    }
                    // A word wider than the line: cut it where it overflows.
                    let mut part = String::new();
                    let mut part_w = 0.0;
                    for c in token.chars() {
                        let cw = measure(c.encode_utf8(&mut [0; 4]));
                        if b.width + part_w + cw > width && (!part.is_empty() || b.content) {
                            if !part.is_empty() {
                                b.pieces.push(piece(&part, part_w));
                                b.width += part_w;
                                b.size = b.size.max(span.size);
                            }
                            lines.push(b.take(default_size));
                            b.wrapped = true;
                            part.clear();
                            part_w = 0.0;
                        }
                        part.push(c);
                        part_w += cw;
                    }
                    if !part.is_empty() {
                        b.pieces.push(piece(&part, part_w));
                        b.width += part_w;
                        b.trailing = 0.0;
                        b.size = b.size.max(span.size);
                        b.content = true;
                    }
                }
            }
        }
    }
    lines.push(b.take(default_size));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> Inl {
        Inl::Text(Span {
            text: text.into(),
            font: Font::Regular,
            size: 10.0,
            color: Color32::BLACK,
            underline: false,
            strike: false,
            bg: None,
            raise: 0.0,
        })
    }

    #[test]
    fn measures_and_encodes_text() {
        assert!((text_width(Font::Regular, 10.0, "Hello") - 22.78).abs() < 0.01);
        assert_eq!(encode('é'), Some(0xE9));
        assert_eq!(encode('’'), Some(0x92));
        assert_eq!(encode('\u{1}'), None);
        assert!(fit(Font::Regular, 10.0, "un texte bien trop long", 40.0).ends_with('…'));
        assert_eq!(fit(Font::Regular, 10.0, "ok", 40.0), "ok");
    }

    #[test]
    fn wraps_words_and_cuts_overlong_ones() {
        let lines = flow(&[span("un deux trois quatre cinq six sept huit")], 60.0, 10.0);
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|l| l.width <= 60.5), "{:?}", lines.iter().map(|l| l.width).collect::<Vec<_>>());
        let lines = flow(&[span("supercalifragilisticexpialidocious")], 50.0, 10.0);
        assert!(lines.len() > 1);
        let lines = flow(&[span("a\nb")], 100.0, 10.0);
        assert_eq!(lines.len(), 2);
        assert_eq!(flow(&[], 100.0, 10.0).len(), 1);
    }

    #[test]
    fn writes_a_well_formed_file() {
        let mut pdf = Pdf::default();
        pdf.page(200.0, 100.0);
        pdf.text(10.0, 20.0, Font::Bold, 12.0, Color32::RED, "Bonjour (le) monde é");
        pdf.fill_rect(5.0, 5.0, 20.0, 10.0, Color32::BLUE);
        pdf.ellipse(30.0, 30.0, 20.0, 10.0, Some(Color32::GREEN), Some((Color32::BLACK, 1.0)));
        pdf.rounded_rect(60.0, 30.0, 20.0, 10.0, 3.0, None, Some((Color32::BLACK, 1.0)));
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 100])));
        let index = pdf.add_image(&img);
        pdf.image(0.0, 0.0, 10.0, 10.0, index);
        pdf.page(200.0, 100.0);
        let bytes = pdf.finish();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/SMask"));
        assert!(text.trim_end().ends_with("%%EOF"));
        // The xref table points at the objects.
        let start: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(bytes[start..].starts_with(b"xref"));
    }
}
