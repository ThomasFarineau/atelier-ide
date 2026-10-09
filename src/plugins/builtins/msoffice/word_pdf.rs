//! Word document to PDF: A4 pages, paragraphs wrapped with the Helvetica metrics of `pdf`,
//! lists, tables and pictures.

use super::{
    Block, Doc, Format, INK, Inline, Kind, LINK, PAPER_BORDER, Para, Table, base_points,
    pdf::{Font, Inl, Pdf, Span, flow},
};
use anyhow::Result;
use eframe::egui::{Align, Color32};
use std::{collections::HashMap, fs, path::Path};

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 56.0;
/// Pixels of a document picture to points.
const PX: f32 = 0.75;

struct Exporter<'a> {
    doc: &'a Doc,
    pdf: Pdf,
    y: f32,
    /// Measuring only: nothing is drawn and pages do not break.
    dry: bool,
    /// Inside a table cell: the row already fits, pages do not break.
    fixed: bool,
    images: HashMap<String, Option<(usize, [f32; 2])>>,
}

pub fn export(doc: &Doc, target: &Path) -> Result<()> {
    let mut e = Exporter {
        doc,
        pdf: Pdf::default(),
        y: MARGIN,
        dry: false,
        fixed: false,
        images: HashMap::new(),
    };
    e.pdf.page(PAGE_W, PAGE_H);
    e.blocks(&doc.blocks, MARGIN, PAGE_W - 2.0 * MARGIN);
    fs::write(target, e.pdf.finish())?;
    Ok(())
}

fn span(kind: Kind, format: &Format, link: bool) -> Span {
    let mut size = format.size.unwrap_or_else(|| base_points(kind));
    if format.sup || format.sub {
        size *= 0.65;
    }
    let color = if link { LINK } else { format.color.unwrap_or(INK) };
    Span {
        text: String::new(),
        font: Font::of(format.bold || kind != Kind::Body, format.italic),
        size,
        color,
        underline: format.underline || link,
        strike: format.strike,
        bg: format.highlight,
        raise: if format.sup {
            size * 0.8
        } else if format.sub {
            -size * 0.25
        } else {
            0.0
        },
    }
}

impl Exporter<'_> {
    fn new_page(&mut self) {
        self.pdf.page(PAGE_W, PAGE_H);
        self.y = MARGIN;
    }

    /// Starts a new page when `height` does not fit below the cursor.
    fn ensure(&mut self, height: f32) {
        if !self.dry && !self.fixed && self.y + height > PAGE_H - MARGIN && self.y > MARGIN + 1.0 {
            self.new_page();
        }
    }

    fn image(&mut self, rel: &str) -> Option<(usize, [f32; 2])> {
        if !self.images.contains_key(rel) {
            let loaded = self.doc.images.get(rel).and_then(|data| image::load_from_memory(data).ok()).map(|img| {
                let size = [img.width() as f32 * PX, img.height() as f32 * PX];
                (self.pdf.add_image(&img), size)
            });
            self.images.insert(rel.to_string(), loaded);
        }
        self.images[rel]
    }

    fn blocks(&mut self, blocks: &[Block], x: f32, width: f32) {
        for block in blocks {
            match block {
                Block::Para(p) => self.para(p, x, width),
                Block::Table(t) => self.table(t, x, width),
            }
        }
    }

    fn para(&mut self, p: &Para, x: f32, width: f32) {
        let heading = p.kind != Kind::Body;
        if heading {
            self.y += 8.0;
        }
        let indent = p.indent.min(width / 2.0);
        let (x0, w) = (x + indent, width - indent);
        let mut items = Vec::new();
        for inline in &p.inlines {
            match inline {
                Inline::Text { text, format, link } => {
                    let mut s = span(p.kind, format, link.is_some());
                    s.text = text.replace('\t', "    ");
                    items.push(Inl::Text(s));
                }
                Inline::Image { rel, size } => {
                    let Some((index, natural)) = self.image(rel) else { continue };
                    let wanted = size.map_or(natural, |s| [s[0] * PX, s[1] * PX]);
                    let fit = (w / wanted[0]).min((PAGE_H - 2.0 * MARGIN) / wanted[1]).min(1.0);
                    items.push(Inl::Image {
                        index,
                        w: wanted[0] * fit,
                        h: wanted[1] * fit,
                    });
                }
            }
        }
        let lines = flow(&items, w, base_points(p.kind));
        for (i, line) in lines.iter().enumerate() {
            self.ensure(line.height);
            if !self.dry {
                if i == 0
                    && let Some(marker) = &p.marker
                {
                    let size = base_points(p.kind);
                    let at = x0 - 3.0 - super::pdf::text_width(Font::Regular, size, marker);
                    self.pdf.text(at, self.y + line.ascent, Font::Regular, size, INK, marker);
                }
                self.pdf.draw_line(line, x0, self.y, w, p.align);
            }
            self.y += line.height;
        }
        self.y += if heading { 4.0 } else { 6.0 };
    }

    fn table(&mut self, t: &Table, x: f32, width: f32) {
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
        let mut relative = t.widths.clone();
        relative.resize(columns, relative.iter().sum::<f32>() / relative.len().max(1) as f32);
        if relative.iter().any(|w| *w <= 0.0) {
            relative = vec![1.0; columns];
        }
        let sum: f32 = relative.iter().sum();
        let widths: Vec<f32> = relative.iter().map(|w| width * w / sum).collect();
        let pad = 4.0;
        for row in &t.rows {
            // Cell rectangles of the row, then the height of the tallest content.
            let mut cells = Vec::new();
            let mut column = 0;
            let mut left = x;
            for cell in row {
                let end = (column + cell.span).min(columns);
                let w: f32 = widths[column.min(columns)..end].iter().sum();
                cells.push((cell, left, w));
                left += w;
                column = end;
            }
            let (was_dry, was_fixed) = (self.dry, self.fixed);
            let saved = self.y;
            self.dry = true;
            let mut content = 0.0f32;
            for (cell, _, w) in &cells {
                self.y = 0.0;
                self.blocks(&cell.blocks, 0.0, (w - 2.0 * pad).max(8.0));
                content = content.max(self.y);
            }
            self.dry = was_dry;
            self.y = saved;
            let height = (content + 2.0 * pad).max(16.0);
            self.ensure(height);
            let top = self.y;
            if !self.dry {
                for (cell, left, w) in &cells {
                    if let Some(fill) = cell.fill {
                        self.pdf.fill_rect(*left, top, *w, height, fill);
                    }
                    self.pdf.stroke_rect(*left, top, *w, height, PAPER_BORDER, 0.6);
                    self.y = top + pad;
                    self.fixed = true;
                    self.blocks(&cell.blocks, left + pad, (w - 2.0 * pad).max(8.0));
                }
            }
            self.fixed = was_fixed;
            self.y = top + height;
        }
        self.y += 6.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtins::msoffice::{Cell, Inline};

    fn text_para(text: &str) -> Block {
        Block::Para(Para {
            kind: Kind::Body,
            align: Align::Min,
            indent: 0.0,
            marker: None,
            inlines: vec![Inline::Text {
                text: text.into(),
                format: Format::default(),
                link: None,
            }],
        })
    }

    #[test]
    fn long_documents_span_several_pages() {
        let mut blocks: Vec<Block> = (0..120).map(|i| text_para(&format!("Paragraphe numéro {i}"))).collect();
        blocks.push(Block::Table(Table {
            widths: vec![1.0, 1.0],
            rows: vec![vec![
                Cell { span: 1, fill: Some(Color32::LIGHT_GRAY), blocks: vec![text_para("A")] },
                Cell { span: 1, fill: None, blocks: vec![text_para("B")] },
            ]],
        }));
        let doc = Doc { blocks, images: HashMap::new(), words: 0 };
        let path = std::env::temp_dir().join(format!("atelier-word-{}.pdf", std::process::id()));
        export(&doc, &path).unwrap();
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-"));
        let pages: usize = text.split("/Count ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(pages >= 3, "{pages} pages");
    }
}
