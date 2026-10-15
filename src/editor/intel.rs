//! More of what a language server knows, in the editor: Parameter Info (Ctrl+P), Quick
//! Documentation (Ctrl+Q), Quick Definition (Ctrl+Shift+I), Refactor This (Ctrl+Alt+Shift+T) and
//! the hierarchies of types (Ctrl+H) and of calls (Ctrl+Alt+H).

use super::{
    EditorState, Options, column_of,
    assist::{Choice, Chooser, Entry, draw_hover},
};
use crate::{
    languages::{
        Hover, Location, Oneshot, QuickFix,
        lsp::extras::{Hierarchy, HierarchyItem, InlayHint, Signature},
    },
    ui::Theme,
};
use eframe::egui::{
    self, Align2, FontId, Key, Modifiers, Order, Pos2, Rect, Stroke, StrokeKind, Ui, pos2,
    text::{CCursor, LayoutJob, TextFormat},
    vec2,
};
use std::{
    fs,
    ops::Range,
    time::{Duration, Instant},
};

/// What a shortcut asked for, to be answered at the next frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Want {
    Signature,
    Documentation,
    Definition,
    Refactor,
    Hierarchy(Hierarchy),
}

/// The call being typed: its signature, kept up to date while the caret moves in it.
struct SignatureState {
    answer: Option<Oneshot<Option<Signature>>>,
    shown: Option<Signature>,
    /// The caret and the revision the signature was asked for.
    asked: (usize, u64),
}

/// A popup next to the caret that goes away when the caret moves or the text changes.
enum Quick {
    Documentation(Hover),
    Code {
        title: String,
        lines: Vec<String>,
    },
}

struct QuickState {
    answer: Option<QuickAnswer>,
    shown: Option<Quick>,
    at: (usize, u64),
}

enum QuickAnswer {
    Documentation(Oneshot<Option<Hover>>),
    Definition(Oneshot<Vec<Location>>),
}

/// A hierarchy open as a list of the chooser: the items by the order of its entries.
struct HierarchyState {
    kind: Hierarchy,
    /// The parents (supertypes, callers) rather than the children.
    up: bool,
    items: Vec<(HierarchyItem, bool)>,
    /// The entry being expanded, and the answer once it is asked for.
    expanding: Option<(usize, Option<Oneshot<Vec<HierarchyItem>>>)>,
}

/// What the hints are for: the revision of the text and the first and last line.
type HintsFor = (u64, usize, usize);

/// The request of the hints: for which text and which lines, and the answer to come.
#[derive(Default)]
pub(super) struct HintsState {
    /// The revision and the lines (first, last) the hints in the editor are for.
    for_text: Option<HintsFor>,
    answer: Option<(Oneshot<Vec<InlayHint>>, HintsFor)>,
    asked: Option<Instant>,
}

/// Lines asked for around the ones shown, so that scrolling does not wait for the server.
const HINT_MARGIN: usize = 60;
/// A new request waits for the typing to pause.
const HINT_DELAY: Duration = Duration::from_millis(350);

#[derive(Default)]
pub(super) struct IntelState {
    pub(super) want: Option<Want>,
    prepare: Option<(Hierarchy, Oneshot<Vec<HierarchyItem>>)>,
    refactors: Option<Oneshot<Vec<QuickFix>>>,
    signature: Option<SignatureState>,
    quick: Option<QuickState>,
    hierarchy: Option<HierarchyState>,
}

fn location_of(item: &HierarchyItem) -> Location {
    Location {
        path: item.path.clone(),
        line: item.line,
        column: item.column,
        end_line: item.line,
        end_column: item.column,
        preview: item.name.clone(),
    }
}

fn entry_of(item: &HierarchyItem, indent: usize) -> Entry {
    let place = item
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| format!("{}:{}", n.to_string_lossy(), item.line + 1))
        .unwrap_or_default();
    Entry {
        label: item.name.clone(),
        detail: if item.detail.is_empty() {
            place
        } else {
            format!("{}  {place}", item.detail)
        },
        kind: Some(item.kind),
        indent,
        choice: Choice::Go(location_of(item)),
    }
}

/// A few lines of the file of `place`, around the line.
fn code_around(place: &Location, text: &str) -> Vec<String> {
    let source = match &place.path {
        Some(path) => fs::read_to_string(path).ok().filter(|t| t.len() <= 4_000_000),
        None => Some(text.to_string()),
    };
    let Some(source) = source else {
        return vec![place.preview.clone()];
    };
    source
        .lines()
        .skip(place.line)
        .take(14)
        .map(|l| l.replace('\t', "    "))
        .collect()
}

impl EditorState {
    /// Keeps the hints of the visible lines up to date: asks the language server once the typing
    /// has paused, takes its answer.
    pub(super) fn hints_frame(&mut self, ui: &Ui, text: &str, opt: &Options, first: usize, last: usize) {
        let Some(assist) = opt.assist.as_ref().filter(|_| opt.inlay_hints) else {
            self.hints.clear();
            self.hints_state = HintsState::default();
            return;
        };
        if let Some((answer, asked)) = &self.hints_state.answer {
            if let Some(hints) = answer.take() {
                // The text moved on meanwhile: the answer is for the text it was asked for.
                let current = asked.0 == self.rev;
                let asked = *asked;
                self.hints_state.answer = None;
                if current {
                    self.hints = hints;
                    self.hints_state.for_text = Some(asked);
                }
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(60));
            }
            return;
        }
        let covered = self.hints_state.for_text.is_some_and(|(rev, a, b)| {
            rev == self.rev && a <= first && last <= b
        });
        if covered {
            return;
        }
        let wait = self.hints_state.asked.map_or(Duration::ZERO, |t| HINT_DELAY.saturating_sub(t.elapsed()));
        let typing = self.activity.elapsed() < HINT_DELAY;
        if !wait.is_zero() || (typing && !self.hints.is_empty()) {
            ui.ctx().request_repaint_after(wait.max(Duration::from_millis(60)));
            return;
        }
        let from = first.saturating_sub(HINT_MARGIN);
        let to = (last + HINT_MARGIN).min(self.lines.len() - 1);
        self.hints_state.asked = Some(Instant::now());
        self.hints_state.answer = Some((
            assist.service.inlay_hints(&assist.doc, text, from, to),
            (self.rev, from, to),
        ));
        ui.ctx().request_repaint_after(Duration::from_millis(60));
    }

    /// A shortcut of this module. Whether the key was taken.
    pub(super) fn intel_key(&mut self, key: Key, m: Modifiers) -> bool {
        let want = match key {
            Key::P if m.command && !m.shift && !m.alt => Want::Signature,
            Key::Q if m.command && !m.shift && !m.alt => Want::Documentation,
            Key::I if m.command && m.shift && !m.alt => Want::Definition,
            Key::T if m.command && m.shift && m.alt => Want::Refactor,
            Key::H if m.command && m.alt && !m.shift => Want::Hierarchy(Hierarchy::Calls),
            Key::H if m.command && !m.alt && !m.shift => Want::Hierarchy(Hierarchy::Types),
            Key::Escape if self.intel.signature.is_some() || self.intel.quick.is_some() => {
                self.intel.signature = None;
                self.intel.quick = None;
                return true;
            }
            _ => return false,
        };
        self.intel.want = Some(want);
        true
    }

    /// The keys of a hierarchy in the list: Right opens an item, Left closes it, Tab turns the
    /// parents into the children and back.
    pub(super) fn intel_chooser_key(&mut self, key: Key) -> bool {
        let Some(state) = &mut self.intel.hierarchy else {
            return false;
        };
        let Some(chooser) = &mut self.assist.chooser else {
            return false;
        };
        let Some(&at) = chooser.shown.get(chooser.selected) else {
            return false;
        };
        match key {
            Key::ArrowRight if state.expanding.is_none() && !state.items[at].1 => {
                state.items[at].1 = true;
                // The request goes out at the next frame, which has the service.
                state.expanding = Some((at, None));
                true
            }
            Key::ArrowLeft => {
                // Closes the children, or goes up to the parent.
                let indent = chooser.entries[at].indent;
                if state.items[at].1 && chooser.entries.get(at + 1).is_some_and(|e| e.indent > indent) {
                    let mut end = at + 1;
                    while chooser.entries.get(end).is_some_and(|e| e.indent > indent) {
                        end += 1;
                    }
                    chooser.entries.drain(at + 1..end);
                    state.items.drain(at + 1..end);
                    state.items[at].1 = false;
                    chooser.shown = (0..chooser.entries.len()).collect();
                } else if let Some(parent) = (0..at).rev().find(|&i| chooser.entries[i].indent < indent) {
                    chooser.selected = parent;
                    chooser.scroll = chooser.scroll.min(parent);
                }
                true
            }
            Key::Tab => {
                state.up = !state.up;
                let root = state.items[0].0.clone();
                chooser.entries = vec![entry_of(&root, 0)];
                chooser.shown = vec![0];
                chooser.selected = 0;
                chooser.scroll = 0;
                state.items = vec![(root, true)];
                state.expanding = Some((0, None));
                chooser.title = Self::hierarchy_title(state.kind, state.up);
                true
            }
            _ => false,
        }
    }

    fn hierarchy_title(kind: Hierarchy, up: bool) -> &'static str {
        match (kind, up) {
            (Hierarchy::Types, true) => tr!("supertypes"),
            (Hierarchy::Types, false) => tr!("subtypes"),
            (Hierarchy::Calls, true) => tr!("callers"),
            (Hierarchy::Calls, false) => tr!("callees"),
        }
    }

    /// Asks for what the shortcuts wanted, and takes the answers.
    pub(super) fn intel_frame(&mut self, ui: &Ui, text: &str, opt: &Options) {
        let Some(assist) = &opt.assist else {
            self.intel = IntelState::default();
            return;
        };
        let (service, doc) = (assist.service, &assist.doc);
        let wait = || ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        let here = (self.cursor, self.rev);

        // ---- What was asked.
        match self.intel.want.take() {
            Some(Want::Signature) => {
                self.intel.quick = None;
                self.intel.signature = Some(SignatureState {
                    answer: Some(service.signature_help(doc, text, self.cursor)),
                    shown: None,
                    asked: here,
                });
            }
            Some(Want::Documentation) => {
                self.intel.signature = None;
                let word = self.word_at(text, self.cursor);
                let at = if word.is_empty() { self.cursor } else { word.start };
                self.intel.quick = Some(QuickState {
                    answer: Some(QuickAnswer::Documentation(service.hover(doc, text, at))),
                    shown: None,
                    at: here,
                });
            }
            Some(Want::Definition) => {
                self.intel.signature = None;
                self.intel.quick = Some(QuickState {
                    answer: Some(QuickAnswer::Definition(service.definition(doc, text, self.cursor))),
                    shown: None,
                    at: here,
                });
            }
            Some(Want::Refactor) => {
                let range = {
                    let selection = self.selection();
                    if selection.is_empty() {
                        self.word_at(text, self.cursor)
                    } else {
                        selection
                    }
                };
                let range = if range.is_empty() {
                    self.cursor..self.cursor
                } else {
                    range
                };
                self.intel.refactors = Some(service.refactorings(doc, text, range));
            }
            Some(Want::Hierarchy(kind)) => {
                self.intel.prepare = Some((kind, service.prepare_hierarchy(doc, text, self.cursor, kind)));
            }
            None => {}
        }

        // ---- Refactor This.
        if let Some(answer) = &self.intel.refactors {
            if let Some(fixes) = answer.take() {
                self.intel.refactors = None;
                if fixes.is_empty() {
                    self.assist.message = Some(tr!("no_refactorings").into());
                } else {
                    let entries = fixes
                        .into_iter()
                        .map(|fix| Entry {
                            label: fix.title.clone(),
                            detail: String::new(),
                            kind: None,
                            indent: 0,
                            choice: Choice::Fix(fix),
                        })
                        .collect();
                    self.assist.popup = None;
                    self.assist.chooser = Some(Chooser::new(tr!("refactor_this"), entries, true));
                }
            } else {
                wait();
            }
        }

        // ---- Parameter Info: kept up to date while the caret moves in the call.
        if let Some(state) = &mut self.intel.signature {
            if let Some(answer) = &state.answer {
                if let Some(result) = answer.take() {
                    state.answer = None;
                    match result {
                        Some(signature) => state.shown = Some(signature),
                        None => {
                            self.intel.signature = None;
                            return self.intel_frame_rest(text, opt, &wait);
                        }
                    }
                } else {
                    wait();
                }
            } else if state.asked != here {
                state.asked = here;
                state.answer = Some(service.signature_help(doc, text, self.cursor));
            }
        }
        self.intel_frame_rest(text, opt, &wait);
    }

    fn intel_frame_rest(&mut self, text: &str, opt: &Options, wait: &dyn Fn()) {
        let Some(assist) = &opt.assist else { return };
        let (service, doc) = (assist.service, &assist.doc);
        let here = (self.cursor, self.rev);

        // ---- Quick Documentation and Quick Definition.
        if let Some(state) = &mut self.intel.quick {
            if state.at != here {
                self.intel.quick = None;
            } else if let Some(answer) = &state.answer {
                let ready = match answer {
                    QuickAnswer::Documentation(a) => a.take().map(|hover| {
                        hover.map(Quick::Documentation)
                    }),
                    QuickAnswer::Definition(a) => a.take().map(|places| {
                        places.first().map(|place| Quick::Code {
                            title: match &place.path {
                                Some(path) => format!(
                                    "{}:{}",
                                    path.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default(),
                                    place.line + 1
                                ),
                                None => trf!("line", line = place.line + 1),
                            },
                            lines: code_around(place, text),
                        })
                    }),
                };
                match ready {
                    Some(Some(quick)) => {
                        state.shown = Some(quick);
                        state.answer = None;
                    }
                    Some(None) => {
                        self.intel.quick = None;
                        self.assist.message = Some(tr!("no_documentation").into());
                    }
                    None => wait(),
                }
            }
        }

        // ---- Hierarchies: the item at the caret, then its first level.
        if let Some((kind, answer)) = &self.intel.prepare {
            if let Some(items) = answer.take() {
                let kind = *kind;
                self.intel.prepare = None;
                match items.into_iter().next() {
                    Some(root) => {
                        let up = kind == Hierarchy::Calls;
                        let mut chooser = Chooser::new(
                            Self::hierarchy_title(kind, up),
                            vec![entry_of(&root, 0)],
                            false,
                        );
                        chooser.centered = true;
                        self.assist.popup = None;
                        self.assist.chooser = Some(chooser);
                        self.intel.hierarchy = Some(HierarchyState {
                            kind,
                            up,
                            items: vec![(root, true)],
                            expanding: Some((0, None)),
                        });
                    }
                    None => self.assist.message = Some(tr!("no_hierarchy").into()),
                }
            } else {
                wait();
            }
        }
        if self.assist.chooser.is_none() {
            self.intel.hierarchy = None;
        }
        if let (Some(state), Some(chooser)) = (&mut self.intel.hierarchy, &mut self.assist.chooser)
            && let Some((at, slot)) = &mut state.expanding
        {
            let at = *at;
            if slot.is_none() {
                *slot = Some(service.expand_hierarchy(doc, &state.items[at].0, state.kind, state.up));
            }
            if let Some(children) = slot.as_ref().and_then(|a| a.take()) {
                state.expanding = None;
                let indent = chooser.entries[at].indent;
                for (n, child) in children.iter().enumerate() {
                    chooser.entries.insert(at + 1 + n, entry_of(child, indent + 1));
                    state.items.insert(at + 1 + n, (child.clone(), false));
                }
                chooser.shown = (0..chooser.entries.len()).collect();
            } else {
                wait();
            }
        }
    }

    /// Draws the signature and the quick views next to the caret.
    pub(super) fn intel_paint(
        &mut self,
        ui: &Ui,
        text: &str,
        opt: &Options,
        origin: Pos2,
        text_left: f32,
        row: f32,
    ) {
        if self.intel.signature.as_ref().is_none_or(|s| s.shown.is_none())
            && self.intel.quick.as_ref().is_none_or(|q| q.shown.is_none())
        {
            return;
        }
        let t = opt.theme;
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            Order::Foreground,
            egui::Id::new("editor_intel"),
        ));
        let screen = ui.ctx().content_rect();
        let line = self.line_of(self.cursor);
        let range = self.line_range(text, line);
        let column = column_of(
            &text[range.clone()],
            self.cursor.saturating_sub(range.start).min(range.len()),
        );
        let galley = self.galley(ui, text, line, opt);
        let column = self.visual_column(text, line, column);
        let caret_x = text_left + galley.pos_from_cursor(CCursor::new(column)).min.x;
        let top = origin.y + self.row_of(line) as f32 * row;
        if let Some(signature) = self.intel.signature.as_ref().and_then(|s| s.shown.as_ref()) {
            draw_signature(&painter, &t, &opt.font, signature, pos2(caret_x, top), screen);
        }
        if let Some(quick) = self.intel.quick.as_ref().and_then(|q| q.shown.as_ref()) {
            let below = pos2(caret_x, top + row);
            match quick {
                Quick::Documentation(hover) => {
                    draw_hover(&painter, &t, &opt.font, &[], Some(hover), below, screen)
                }
                Quick::Code { title, lines } => {
                    draw_code(&painter, &t, &opt.font, title, lines, below, screen)
                }
            }
        }
    }
}

/// The signature above the caret, the parameter being typed in the accent color.
fn draw_signature(
    painter: &egui::Painter,
    t: &Theme,
    font: &FontId,
    signature: &Signature,
    caret_top: Pos2,
    screen: Rect,
) {
    let size = FontId::monospace((font.size - 1.0).max(11.0));
    let mut job = LayoutJob::default();
    let chars: Vec<char> = signature.label.chars().collect();
    let active: Option<Range<usize>> = signature
        .active
        .and_then(|i| signature.parameters.get(i).cloned());
    let part = |from: usize, to: usize| chars[from.min(chars.len())..to.min(chars.len())].iter().collect::<String>();
    let plain = TextFormat::simple(size.clone(), t.text);
    match active {
        Some(range) => {
            job.append(&part(0, range.start), 0.0, plain.clone());
            job.append(
                &part(range.start, range.end),
                0.0,
                TextFormat {
                    underline: Stroke::new(1.0_f32, t.accent),
                    ..TextFormat::simple(size.clone(), t.accent)
                },
            );
            job.append(&part(range.end, chars.len()), 0.0, plain);
        }
        None => job.append(&signature.label, 0.0, plain),
    }
    job.wrap.max_width = (screen.width() - 40.0).max(200.0);
    let galley = painter.layout_job(job);
    let doc = (!signature.doc.is_empty()).then(|| {
        painter.layout(
            signature.doc.lines().next().unwrap_or("").to_string(),
            FontId::proportional(12.0),
            t.text_secondary,
            screen.width() - 60.0,
        )
    });
    let counter = (signature.count > 1)
        .then(|| format!("{}/{}", signature.index + 1, signature.count));
    let width = galley.size().x.max(doc.as_ref().map_or(0.0, |d| d.size().x)) + 20.0;
    let height = galley.size().y + doc.as_ref().map_or(0.0, |d| d.size().y + 4.0) + 12.0;
    let mut x = caret_top.x.min(screen.right() - width - 8.0).max(screen.left() + 8.0);
    if x + width > screen.right() {
        x = screen.left() + 8.0;
    }
    let y = (caret_top.y - height - 4.0).max(screen.top() + 4.0);
    let rect = Rect::from_min_size(pos2(x, y), vec2(width, height));
    painter.rect(rect, 5.0, t.popup, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
    painter.galley(pos2(rect.left() + 10.0, rect.top() + 6.0), galley.clone(), t.text);
    if let Some(doc) = doc {
        painter.galley(
            pos2(rect.left() + 10.0, rect.top() + 6.0 + galley.size().y + 4.0),
            doc,
            t.text_secondary,
        );
    }
    if let Some(counter) = counter {
        painter.text(
            pos2(rect.right() - 6.0, rect.top() + 4.0),
            Align2::RIGHT_TOP,
            counter,
            FontId::proportional(10.5),
            t.text_secondary,
        );
    }
}

/// A few lines of code in a popup: Quick Definition.
fn draw_code(
    painter: &egui::Painter,
    t: &Theme,
    font: &FontId,
    title: &str,
    lines: &[String],
    at: Pos2,
    screen: Rect,
) {
    let size = FontId::monospace((font.size - 1.0).max(11.0));
    let header = painter.layout_no_wrap(title.to_string(), FontId::proportional(12.0), t.text_secondary);
    let body = painter.layout(
        lines.join("\n"),
        size,
        t.text,
        (screen.width() - 60.0).max(200.0),
    );
    let width = body.size().x.max(header.size().x).clamp(200.0, screen.width() - 40.0) + 20.0;
    let height = header.size().y + body.size().y + 20.0;
    let x = at.x.min(screen.right() - width - 8.0).max(screen.left() + 8.0);
    let y = if at.y + height + 8.0 > screen.bottom() {
        (at.y - height - 24.0).max(screen.top())
    } else {
        at.y + 4.0
    };
    let rect = Rect::from_min_size(pos2(x, y), vec2(width, height));
    painter.rect(rect, 5.0, t.popup, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
    painter.galley(pos2(rect.left() + 10.0, rect.top() + 6.0), header.clone(), t.text_secondary);
    painter.galley(
        pos2(rect.left() + 10.0, rect.top() + 12.0 + header.size().y),
        body,
        t.text,
    );
}
