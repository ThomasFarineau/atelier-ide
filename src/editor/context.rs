//! Where the caret is in the code: the sticky lines (the lines that open the blocks around the
//! top of the window stay on top, as IntelliJ's) and the breadcrumbs (file > class > function)
//! above the editor.

use super::{EditorState, Options};
use crate::languages::{Oneshot, Symbol};
use eframe::egui::{self, Align2, FontId, Rect, Sense, Ui, pos2, vec2};
use std::time::{Duration, Instant};

/// Lines kept on top at most.
const STICKY_MAX: usize = 5;
/// Height of the breadcrumbs.
pub(super) const CRUMB_HEIGHT: f32 = 22.0;
/// The symbols are asked for again once the typing has paused this long.
const SYMBOLS_DELAY: Duration = Duration::from_millis(500);

/// The symbols of the document, for the breadcrumbs.
#[derive(Default)]
pub(super) struct CrumbState {
    rev: Option<u64>,
    symbols: Vec<Symbol>,
    answer: Option<(Oneshot<Vec<Symbol>>, u64)>,
    asked: Option<Instant>,
}

impl EditorState {
    /// The line a block starts on: the one that opens its brace, or, when that one only closes a
    /// parenthesis (`) {` after a long signature), the line that opened the parenthesis.
    fn block_start(&self, text: &str, line: usize) -> usize {
        let parens = |l: usize| -> isize {
            self.line(text, l)
                .chars()
                .map(|c| match c {
                    '(' | '[' => 1,
                    ')' | ']' => -1,
                    _ => 0,
                })
                .sum()
        };
        let mut sum = parens(line);
        let mut at = line;
        while sum < 0 && at > 0 {
            at -= 1;
            sum += parens(at);
        }
        at
    }

    /// The first lines of the blocks (between braces) that hold `line` and start above it,
    /// outermost first.
    pub(super) fn block_headers(&self, text: &str, line: usize) -> Vec<usize> {
        let mut headers: Vec<usize> = self
            .regions
            .iter()
            .filter(|r| r.first < line && line <= r.last)
            .filter(|r| text.as_bytes().get(r.open) == Some(&b'{'))
            .map(|r| self.block_start(text, r.first))
            .collect();
        headers.sort_unstable();
        headers.dedup();
        headers
    }

    /// The lines to keep on top when `top_row` is the first row shown whole: they hide the rows
    /// under them, so the headers are those of the first line that stays visible.
    pub(super) fn sticky_lines(&self, text: &str, top_row: usize) -> Vec<usize> {
        if self.regions.is_empty() || top_row == 0 {
            return Vec::new();
        }
        let mut count = 0;
        let mut found = Vec::new();
        for _ in 0..=STICKY_MAX {
            let row = (top_row + count).min(self.row_count().saturating_sub(1));
            let mut headers = self.block_headers(text, self.line_at(row));
            // The innermost ones when there are too many.
            let extra = headers.len().saturating_sub(STICKY_MAX);
            headers.drain(..extra);
            if headers.len() == count {
                return headers;
            }
            count = headers.len();
            found = headers;
        }
        found
    }

    /// Keeps the symbols of the document for the breadcrumbs.
    pub(super) fn crumbs_frame(&mut self, ui: &Ui, text: &str, opt: &Options) {
        let Some(assist) = opt.assist.as_ref().filter(|_| opt.breadcrumbs) else {
            self.crumbs = CrumbState::default();
            return;
        };
        if let Some((answer, rev)) = &self.crumbs.answer {
            if let Some(symbols) = answer.take() {
                let rev = *rev;
                self.crumbs.answer = None;
                self.crumbs.symbols = symbols;
                self.crumbs.rev = Some(rev);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(60));
            }
            return;
        }
        if self.crumbs.rev == Some(self.rev) {
            return;
        }
        let typing = self.activity.elapsed() < SYMBOLS_DELAY && self.crumbs.rev.is_some();
        if typing {
            ui.ctx().request_repaint_after(SYMBOLS_DELAY);
            return;
        }
        self.crumbs.asked = Some(Instant::now());
        self.crumbs.answer = Some((assist.service.symbols(&assist.doc, text), self.rev));
        ui.ctx().request_repaint_after(Duration::from_millis(60));
    }

    /// The symbols around the caret, outermost first: each with the offset of its name.
    fn enclosing_symbols(&self, text: &str) -> Vec<(String, usize)> {
        let line = self.line_of(self.cursor);
        let mut chain: Vec<&Symbol> = Vec::new();
        for symbol in &self.crumbs.symbols {
            if symbol.range.start > self.cursor {
                break;
            }
            // The block a symbol opens: the first one that opens after its name, a few lines on
            // (a signature may take several lines).
            let name_line = self.line_of(symbol.range.start);
            let body = self
                .regions
                .iter()
                .filter(|r| r.open >= symbol.range.end)
                .filter(|r| text.as_bytes().get(r.open) == Some(&b'{'))
                .filter(|r| r.first <= name_line + 8)
                .min_by_key(|r| r.open);
            if body.is_some_and(|r| r.first <= line && line <= r.last) {
                chain.truncate(symbol.depth);
                chain.push(symbol);
            }
        }
        chain
            .into_iter()
            .map(|s| (s.name.clone(), s.range.start))
            .collect()
    }

    /// Draws the breadcrumbs and takes a click on one of them: the caret goes there.
    pub(super) fn show_crumbs(&mut self, ui: &mut Ui, text: &str, opt: &Options) {
        let t = opt.theme;
        let (rect, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), CRUMB_HEIGHT),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, t.island);
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0_f32, t.separator),
        );
        let file = opt
            .assist
            .as_ref()
            .and_then(|a| a.doc.path.as_ref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut parts: Vec<(String, Option<usize>)> = vec![(file, None)];
        parts.extend(
            self.enclosing_symbols(text)
                .into_iter()
                .map(|(name, at)| (name, Some(at))),
        );
        let font = FontId::proportional(12.0);
        let mut x = rect.left() + 12.0;
        let last = parts.len() - 1;
        for (i, (name, at)) in parts.into_iter().enumerate() {
            if name.is_empty() {
                continue;
            }
            let color = if i == last { t.text } else { t.text_secondary };
            let galley = painter.layout_no_wrap(name, font.clone(), color);
            let bounds = Rect::from_min_size(pos2(x, rect.top()), vec2(galley.size().x, CRUMB_HEIGHT));
            if let Some(at) = at {
                let response = ui.interact(bounds, egui::Id::new(("crumb", i)), Sense::click());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    self.select(at, at);
                }
            }
            painter.galley(
                pos2(x, rect.center().y - galley.size().y / 2.0),
                galley.clone(),
                color,
            );
            x += galley.size().x + 6.0;
            if i != last {
                painter.text(
                    pos2(x, rect.center().y),
                    Align2::LEFT_CENTER,
                    "›",
                    font.clone(),
                    t.text_secondary,
                );
                x += 14.0;
            }
        }
    }
}
