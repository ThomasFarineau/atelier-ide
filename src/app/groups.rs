//! Editor groups, as in IntelliJ: the editor splits right or down as many times as wanted, each
//! part with tabs of its own, and a tab dragged out of the window opens a window of its own,
//! where the editor splits again.
//!
//! The main group is the editor of the window: every open file is a tab there, with all that
//! the editor does. The other groups show some of those files, each with a view of its own
//! (caret, scroll, folds); the text is the document's, so what is typed in one shows in all.
//! Splitting copies the tab into the new group, as Split Right does in IntelliJ; dragging a tab
//! of another group moves it. Closing a tab of another group leaves the file open; closing the
//! file closes its tabs everywhere.

use super::{Ide, split::SplitPane};
use crate::terminal_layout::Node;
use crate::ui;
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, ViewportId, pos2, vec2,
};
use std::path::{Path, PathBuf};

/// The id of the main group, always in the main window.
pub const MAIN: u64 = 0;
/// Space between two groups, where the divider is dragged.
const GAP: f32 = 5.0;
const BAR: f32 = 32.0;

/// A part of the editor with tabs of its own.
pub struct Group {
    pub id: u64,
    pub tabs: Vec<SplitPane>,
    pub active: usize,
}

/// A window of its own with editor groups in it.
pub struct Window {
    pub id: ViewportId,
    pub layout: Node<u64>,
    /// Where it opens on the screen; only for its first frame.
    position: Option<egui::Pos2>,
}

/// What the mouse drags: a tab of the main group (by document) or of another group.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TabDrag {
    Main(usize),
    Group(u64, usize),
}

/// Where a dragged tab goes when it is dropped on a group.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// The side of `rect` the pointer is on: its middle, or the nearest edge.
    pub fn at(rect: Rect, p: egui::Pos2) -> Side {
        let x = (p.x - rect.left()) / rect.width().max(1.0);
        let y = (p.y - rect.top()) / rect.height().max(1.0);
        if (0.3..=0.7).contains(&x) && (0.3..=0.7).contains(&y) {
            return Side::Center;
        }
        let edges = [
            (x, Side::Left),
            (1.0 - x, Side::Right),
            (y, Side::Top),
            (1.0 - y, Side::Bottom),
        ];
        edges
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(Side::Center, |(_, side)| side)
    }

    /// The part of `rect` the dropped tab will take.
    fn area(self, rect: Rect) -> Rect {
        let c = rect.center();
        match self {
            Side::Center => rect,
            Side::Left => Rect::from_min_max(rect.min, pos2(c.x, rect.bottom())),
            Side::Right => Rect::from_min_max(pos2(c.x, rect.top()), rect.max),
            Side::Top => Rect::from_min_max(rect.min, pos2(rect.right(), c.y)),
            Side::Bottom => Rect::from_min_max(pos2(rect.left(), c.y), rect.max),
        }
    }
}

impl Ide {
    fn new_group(&mut self, path: PathBuf, cursor: usize) -> u64 {
        self.group_count += 1;
        let id = self.group_count;
        self.groups.push(Group {
            id,
            tabs: vec![SplitPane::new(path, cursor)],
            active: 0,
        });
        id
    }

    fn group_mut(&mut self, id: u64) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The layout a group is in: the main window's, or that of a window of its own.
    fn layout_of(&mut self, group: u64) -> Option<&mut Node<u64>> {
        if group == MAIN || self.editor_layout.contains(&|g: &u64| *g == group) {
            return Some(&mut self.editor_layout);
        }
        self.windows
            .iter_mut()
            .find(|w| w.layout.contains(&|g: &u64| *g == group))
            .map(|w| &mut w.layout)
    }

    /// A new group beside `target` (or above, below…) showing `path`.
    pub(super) fn split_group(&mut self, target: u64, path: PathBuf, cursor: usize, side: Side) {
        let id = self.new_group(path, cursor);
        let (stacked, before) = match side {
            Side::Left => (false, true),
            Side::Top => (true, true),
            Side::Bottom => (true, false),
            Side::Right | Side::Center => (false, false),
        };
        let placed = self.layout_of(target).is_some_and(|layout| {
            layout.split_with(&|g: &u64| *g == target, stacked, before, Node::Leaf(id))
        });
        if !placed {
            let _ = self.editor_layout.split_with(
                &|g: &u64| *g == MAIN,
                stacked,
                before,
                Node::Leaf(id),
            );
        }
    }

    /// Split Right (or Down) of the menus: the file of tab `index` of the main group in a new
    /// group beside the main one.
    pub(super) fn split_doc(&mut self, index: usize, down: bool) {
        let Some(doc) = self.documents.get(index) else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            self.status = tr!("split_needs_file").into();
            return;
        };
        let cursor = doc.view.cursor;
        let side = if down { Side::Bottom } else { Side::Right };
        self.split_group(MAIN, path, cursor, side);
    }

    /// Split Right (or Down) of the active file.
    pub(super) fn split_to(&mut self, down: bool) {
        self.split_doc(self.active, down);
    }

    /// Closes the groups other than the main one in the main window (Unsplit All).
    pub(super) fn close_splits(&mut self) {
        let ids: Vec<u64> = self
            .editor_layout
            .leaves()
            .into_iter()
            .copied()
            .filter(|g| *g != MAIN)
            .collect();
        self.editor_layout = Node::Leaf(MAIN);
        self.groups.retain(|g| !ids.contains(&g.id));
    }

    pub(super) fn has_splits(&self) -> bool {
        self.editor_layout.leaves().len() > 1
    }

    /// A window of its own with `path` in it, at `position` on the screen when known.
    pub(super) fn open_window_with(
        &mut self,
        path: PathBuf,
        cursor: usize,
        position: Option<egui::Pos2>,
    ) {
        let id = self.new_group(path, cursor);
        self.window_count += 1;
        self.windows.push(Window {
            id: ViewportId::from_hash_of(("editor_window", self.window_count)),
            layout: Node::Leaf(id),
            position,
        });
    }

    /// Open in New Window of the menu of a tab of the main group.
    pub(super) fn detach(&mut self, index: usize, position: Option<egui::Pos2>) {
        let Some(doc) = self.documents.get(index) else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            self.status = tr!("split_needs_file").into();
            return;
        };
        let cursor = doc.view.cursor;
        self.open_window_with(path, cursor, position);
    }

    /// Takes tab `index` out of group `id`; the group goes away when it has no tab left.
    fn take_tab(&mut self, id: u64, index: usize) -> Option<SplitPane> {
        let group = self.group_mut(id)?;
        if index >= group.tabs.len() {
            return None;
        }
        let pane = group.tabs.remove(index);
        group.active = group.active.min(group.tabs.len().saturating_sub(1));
        if group.tabs.is_empty() {
            self.remove_group(id);
        }
        Some(pane)
    }

    fn remove_group(&mut self, id: u64) {
        self.groups.retain(|g| g.id != id);
        if let Some(layout) = self.layout_of(id) {
            if matches!(layout, Node::Leaf(g) if *g == id) {
                *layout = Node::Empty;
            } else {
                layout.remove(&|g: &u64| *g == id);
            }
        }
        self.windows.retain(|w| !matches!(w.layout, Node::Empty));
    }

    /// Tabs whose file was closed go away, and the groups left without tabs.
    fn prune_groups(&mut self) {
        let open: Vec<PathBuf> = self
            .documents
            .iter()
            .filter_map(|d| d.path.clone())
            .collect();
        let mut empty = Vec::new();
        for group in &mut self.groups {
            group.tabs.retain(|t| open.contains(&t.path));
            group.active = group.active.min(group.tabs.len().saturating_sub(1));
            if group.tabs.is_empty() {
                empty.push(group.id);
            }
        }
        for id in empty {
            self.remove_group(id);
        }
    }

    /// Draws the groups of `layout` in `rect` (the main window's when `window` is None) and the
    /// dividers between them. Returns the place of the main group.
    pub(super) fn layout_ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        window: Option<usize>,
    ) -> Option<Rect> {
        if window.is_none() {
            // Files closed since: their tabs go, and the groups left empty.
            self.prune_groups();
        }
        let layout = match window {
            None => std::mem::replace(&mut self.editor_layout, Node::Empty),
            Some(w) => std::mem::replace(&mut self.windows[w].layout, Node::Empty),
        };
        let mut layout = layout;
        let (rects, dividers) = layout.layout(rect, GAP);
        let ids: Vec<u64> = layout.leaves().into_iter().copied().collect();
        let viewport = ui.ctx().viewport_id();
        let mut main = None;
        let targets = self.drop_targets.entry(viewport).or_default();
        targets.clear();
        targets.extend(ids.iter().copied().zip(rects.iter().copied()));
        for (id, area) in ids.iter().zip(&rects) {
            if *id == MAIN {
                main = Some(*area);
            } else {
                self.group_ui(ui, *id, *area);
            }
        }
        // Dragging a divider changes the share of the two sides.
        let t = self.theme(ui.ctx());
        let mut moves = Vec::new();
        for divider in &dividers {
            let response = ui.interact(
                divider.rect.expand(2.0),
                egui::Id::new(("editor_divider", viewport, divider.path.clone())),
                Sense::drag(),
            );
            let icon = if divider.stacked {
                egui::CursorIcon::ResizeVertical
            } else {
                egui::CursorIcon::ResizeHorizontal
            };
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(icon);
                ui.painter()
                    .rect_filled(divider.rect, 0.0, t.accent.gamma_multiply(0.5));
            }
            if response.dragged()
                && let Some(p) = response.interact_pointer_pos()
            {
                let at = if divider.stacked { p.y } else { p.x };
                moves.push((divider.path.clone(), divider.ratio_at(at, GAP)));
            }
        }
        for (path, ratio) in moves {
            if let Some(slot) = layout.ratio_mut(&path) {
                *slot = ratio;
            }
        }
        match window {
            None => self.editor_layout = layout,
            Some(w) => self.windows[w].layout = layout,
        }
        main
    }

    /// A group other than the main one: its tabs, then the editor of the active one.
    fn group_ui(&mut self, ui: &mut egui::Ui, id: u64, rect: Rect) {
        let t = self.theme(ui.ctx());
        let Some(gi) = self.groups.iter().position(|g| g.id == id) else {
            return;
        };
        let viewport = ui.ctx().viewport_id();
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        ui.set_clip_rect(rect);
        ui.painter().rect_filled(rect, 0.0, t.island);
        // The tabs.
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), BAR));
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            Stroke::new(1.0_f32, t.separator),
        );
        let mut x = bar.left() + 4.0;
        let mut select = None;
        let mut close = None;
        let mut action: Option<(usize, GroupAction)> = None;
        let active = self.groups[gi].active;
        let titles: Vec<(PathBuf, String, bool)> = self.groups[gi]
            .tabs
            .iter()
            .map(|tab| {
                let doc = self
                    .documents
                    .iter()
                    .find(|d| d.path.as_ref() == Some(&tab.path));
                let title = doc.map_or_else(|| file_name(&tab.path), |d| d.title());
                (tab.path.clone(), title, doc.is_some_and(|d| d.dirty()))
            })
            .collect();
        for (i, (path, title, dirty)) in titles.iter().enumerate() {
            let galley =
                ui.painter()
                    .layout_no_wrap(title.clone(), FontId::proportional(12.5), t.text);
            let width = 8.0 + 16.0 + 5.0 + galley.size().x + 6.0 + 16.0 + 6.0;
            if x + width > bar.right() - 4.0 && i > active {
                break;
            }
            let tab = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(width, BAR - 6.0));
            x += width + 2.0;
            let response = ui.interact(
                tab,
                egui::Id::new(("group_tab", id, i)),
                Sense::click_and_drag(),
            );
            if i == active {
                ui.painter().rect(
                    tab,
                    5.0,
                    t.frame,
                    Stroke::new(1.0_f32, t.border),
                    StrokeKind::Inside,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(tab, 5.0, t.hover);
            }
            let icon = Rect::from_min_size(
                pos2(tab.left() + 8.0, tab.center().y - 8.0),
                vec2(16.0, 16.0),
            );
            egui::Image::new(self.file_icon(ui.ctx(), path)).paint_at(&ui, icon);
            ui.painter().galley(
                pos2(icon.right() + 5.0, tab.center().y - galley.size().y / 2.0),
                galley,
                t.text,
            );
            let close_rect = Rect::from_min_size(
                pos2(tab.right() - 22.0, tab.center().y - 8.0),
                vec2(16.0, 16.0),
            );
            let close_response = ui.interact(
                close_rect,
                egui::Id::new(("group_tab_close", id, i)),
                Sense::click(),
            );
            if *dirty && !response.hovered() && !close_response.hovered() {
                ui.painter()
                    .circle_filled(close_rect.center(), 3.5, t.text_secondary);
            } else if i == active || response.hovered() || close_response.hovered() {
                ui::paint_icon(
                    &ui,
                    if close_response.hovered() {
                        "closeSmallHovered"
                    } else {
                        "closeSmall"
                    },
                    close_rect,
                );
            }
            if close_response.clicked() || response.middle_clicked() {
                close = Some(i);
            } else if response.clicked() {
                select = Some(i);
            }
            if response.dragged() && ui.input(|inp| inp.pointer.is_decidedly_dragging()) {
                self.tab_drag = Some((TabDrag::Group(id, i), viewport));
            }
            let response =
                response.on_hover_text(crate::project::simplify(path).display().to_string());
            response.context_menu(|ui| {
                ui::menu_begin(ui);
                for (icon, label, side) in [
                    ("sideBySide", tr!("split_right"), Side::Right),
                    ("unified", tr!("split_down"), Side::Bottom),
                ] {
                    if ui::menu_item(ui, Some(icon), label, None, false).clicked() {
                        action = Some((i, GroupAction::Split(side)));
                        ui.close();
                    }
                }
                if ui::menu_item(
                    ui,
                    Some("openNewTab"),
                    tr!("open_in_new_window"),
                    None,
                    false,
                )
                .clicked()
                {
                    action = Some((i, GroupAction::Window));
                    ui.close();
                }
                ui::menu_separator(ui);
                if ui::menu_item(ui, Some("closeSmall"), tr!("close"), None, false).clicked() {
                    close = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(i) = select {
            self.groups[gi].active = i;
        }
        // The editor of the active tab.
        let body = Rect::from_min_max(pos2(rect.left(), bar.bottom() + 1.0), rect.max);
        let active = self.groups[gi].active;
        let main_path = self.documents.get(self.active).and_then(|d| d.path.clone());
        let uncolored = self.uncolored_languages();
        let font = FontId::monospace(self.settings.font_size);
        if let Some(path) = self.groups[gi].tabs.get(active).map(|p| p.path.clone())
            && let Some(index) = self
                .documents
                .iter()
                .position(|d| d.path.as_ref() == Some(&path))
        {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            let highlight = !uncolored.contains(&self.documents[index].language.as_str());
            // The language service follows one view of a file: not the one of the main group.
            let service = (main_path.as_ref() != Some(&path)).then_some(&*self.languages);
            let pane = &mut self.groups[gi].tabs[active];
            let doc = &mut self.documents[index];
            super::split::show(
                &mut child,
                pane,
                doc,
                service,
                false,
                highlight,
                &self.settings,
                t,
                font,
            );
        }
        if let Some(i) = close {
            self.take_tab(id, i);
        } else if let Some((i, what)) = action
            && let Some(tab) = self.groups[gi].tabs.get(i)
        {
            let (path, cursor) = (tab.path.clone(), tab.view.cursor);
            match what {
                GroupAction::Split(side) => self.split_group(id, path, cursor, side),
                GroupAction::Window => {
                    self.take_tab(id, i);
                    self.open_window_with(path, cursor, None);
                }
            }
        }
    }

    /// Ends or shows the drag of a tab in the window of `viewport`: the side of the group under
    /// the pointer, or a window of its own when the pointer is out of the window.
    pub(super) fn group_drag_ui(&mut self, ctx: &egui::Context) {
        let viewport = ctx.viewport_id();
        let Some((drag, from)) = self.tab_drag else {
            return;
        };
        if from != viewport {
            return;
        }
        let t = self.theme(ctx);
        let latest = ctx.input(|i| i.pointer.latest_pos());
        let screen = ctx.content_rect();
        let outside = latest.is_some_and(|p| !screen.shrink(2.0).contains(p));
        // A tab of the main group dragged along its own bar is moved there, not dropped here.
        let over_bar = matches!(drag, TabDrag::Main(_))
            && self
                .main_bar
                .zip(latest)
                .is_some_and(|(b, p)| b.contains(p));
        let target = latest.filter(|_| !outside && !over_bar).and_then(|p| {
            self.drop_targets
                .get(&viewport)?
                .iter()
                .find(|(_, r)| r.contains(p))
                .map(|(g, r)| (*g, *r, Side::at(*r, p)))
        });
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("group_drop"),
        ));
        if let Some((group, rect, side)) = target {
            // Dropped in its own group's middle, a tab stays where it is.
            let same = matches!((drag, group), (TabDrag::Group(g, _), t) if g == t)
                && side == Side::Center
                || matches!(drag, TabDrag::Main(_)) && group == MAIN && side == Side::Center;
            if !same {
                let area = side.area(rect).shrink(4.0);
                painter.rect(
                    area,
                    6.0,
                    t.accent.gamma_multiply(0.18),
                    Stroke::new(2.0_f32, t.accent),
                    StrokeKind::Inside,
                );
            }
        }
        if outside && let Some(p) = latest {
            let rect = Rect::from_min_size(
                p.clamp(screen.min + vec2(8.0, 8.0), screen.max - vec2(230.0, 40.0)),
                vec2(220.0, 30.0),
            );
            painter.rect_filled(rect, 6.0, t.accent);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr!("open_in_new_window"),
                ui::semibold(13.0),
                Color32::WHITE,
            );
        }
        let released = ctx.input(|i| i.pointer.any_released());
        if !released && ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.tab_drag = None;
        if !released && !outside {
            return;
        }
        // What is dragged: its file and caret.
        let source = match drag {
            TabDrag::Main(i) => self
                .documents
                .get(i)
                .and_then(|d| Some((d.path.clone()?, d.view.cursor))),
            TabDrag::Group(g, i) => self
                .groups
                .iter()
                .find(|x| x.id == g)
                .and_then(|x| x.tabs.get(i))
                .map(|p| (p.path.clone(), p.view.cursor)),
        };
        let Some((path, cursor)) = source else { return };
        // A tab of another group moves: it leaves its group.
        let take = |ide: &mut Ide| {
            if let TabDrag::Group(g, i) = drag {
                ide.take_tab(g, i);
            }
        };
        if outside {
            let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min));
            let position = origin
                .zip(latest)
                .map(|(o, p)| o + p.to_vec2() - vec2(40.0, 20.0));
            take(self);
            self.open_window_with(path, cursor, position);
            return;
        }
        let Some((group, _, side)) = target else {
            return;
        };
        match (side, group) {
            (Side::Center, MAIN) => {
                take(self);
                if let Some(i) = self
                    .documents
                    .iter()
                    .position(|d| d.path.as_ref() == Some(&path))
                {
                    self.active = i;
                }
            }
            (Side::Center, g) => {
                if matches!(drag, TabDrag::Group(from, _) if from == g) {
                    return;
                }
                take(self);
                if let Some(target) = self.group_mut(g) {
                    match target.tabs.iter().position(|p| p.path == path) {
                        Some(i) => target.active = i,
                        None => {
                            target.tabs.push(SplitPane::new(path, cursor));
                            target.active = target.tabs.len() - 1;
                        }
                    }
                }
            }
            (side, g) => {
                // A group alone in its own layout cannot be split by its only tab.
                if matches!(drag, TabDrag::Group(from, i) if from == g
                    && self.group_mut(g).is_some_and(|x| x.tabs.len() == 1 && i == 0))
                {
                    return;
                }
                take(self);
                self.split_group(g, path, cursor, side);
            }
        }
    }

    /// The windows of their own, each with its groups.
    pub(super) fn editor_windows(&mut self, ctx: &egui::Context) {
        self.prune_groups();
        let t = self.theme(ctx);
        let mut closed = Vec::new();
        for w in 0..self.windows.len() {
            let id = self.windows[w].id;
            // Named after the file of its first group.
            let title = self.windows[w]
                .layout
                .leaves()
                .first()
                .and_then(|g| self.groups.iter().find(|x| x.id == **g))
                .and_then(|g| g.tabs.get(g.active))
                .map_or_else(
                    || "Atelier IDE".to_string(),
                    |p| format!("{} – Atelier IDE", file_name(&p.path)),
                );
            let mut builder = egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([900.0, 700.0])
                .with_min_inner_size([320.0, 200.0]);
            if let Some(position) = self.windows[w].position.take() {
                builder = builder.with_position(position);
            }
            let mut close = false;
            ctx.show_viewport_immediate(id, builder, |ctx, class| {
                let draw = |ide: &mut Ide, ui: &mut egui::Ui| {
                    let rect = ui.max_rect();
                    ide.layout_ui(ui, rect, Some(w));
                };
                if class == egui::ViewportClass::Embedded {
                    // A backend without windows of its own: a window inside the main one.
                    let mut open = true;
                    egui::Window::new("Atelier IDE")
                        .id(egui::Id::new(("editor_window", w)))
                        .open(&mut open)
                        .default_size([700.0, 500.0])
                        .show(ctx, |ui| draw(self, ui));
                    close |= !open;
                } else {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().fill(t.island))
                        .show(ctx, |ui| draw(self, ui));
                    self.group_drag_ui(ctx);
                    close |= ctx.input(|i| i.viewport().close_requested());
                }
            });
            if close {
                closed.push(w);
            }
        }
        for w in closed.into_iter().rev() {
            let window = self.windows.remove(w);
            let ids: Vec<u64> = window.layout.leaves().into_iter().copied().collect();
            self.groups.retain(|g| !ids.contains(&g.id));
        }
    }
}

enum GroupAction {
    Split(Side),
    Window,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../../test/app/groups/tests.rs"]
mod tests;
