//! Git user interface, drawn like IntelliJ's New UI: the Commit and Git tool windows, the branch
//! popup, the diff viewer, the merge conflict resolver and the dialogs.

use super::*;
use crate::plugins::builtins::git::conflict::Choice;
use crate::plugins::builtins::git::diff::RowKind;
use crate::plugins::builtins::git::graph::Edge;
use crate::plugins::builtins::git::repo::{Commit, RefKind};
use crate::ui as kit;
use egui::{
    Align, Align2, FontId, Frame, Id, Layout, Margin, Popup, Pos2, Rect, Response, RichText, Sense,
    Stroke, TextEdit, Ui, Vec2, epaint::CubicBezierShape, pos2, vec2,
};

/// Lane colors of the log graph, close to IntelliJ's.
const LANES: [Color32; 8] = [
    Color32::from_rgb(0x5C, 0x9D, 0xF2),
    Color32::from_rgb(0x5F, 0xB8, 0x65),
    Color32::from_rgb(0xE5, 0xA4, 0x4F),
    Color32::from_rgb(0xB0, 0x7F, 0xE8),
    Color32::from_rgb(0x3F, 0xBC, 0xB4),
    Color32::from_rgb(0xE5, 0x68, 0x9B),
    Color32::from_rgb(0xC9, 0xB4, 0x4E),
    Color32::from_rgb(0xE0, 0x6B, 0x60),
];
/// Horizontal distance between two lanes of the graph.
const LANE_W: f32 = 14.0;
const GROUPS: [(&str, Kind); 3] = [
    ("Conflits de fusion", Kind::Conflicted),
    ("Modifications", Kind::Modified),
    ("Fichiers non versionnés", Kind::Untracked),
];

// ---- Small drawing helpers ---------------------------------------------------------------

fn body() -> FontId {
    FontId::proportional(13.0)
}

fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke::new(width, color)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

fn short(hash: &str) -> &str {
    hash.get(..8).unwrap_or(hash)
}

fn file_name(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (name, dir),
        None => (path, ""),
    }
}

fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.into(), font, Color32::WHITE)
        .size()
        .x
}

/// The text shortened to `max` characters with an ellipsis.
fn elide_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Text ending before `right`, shortened with an ellipsis when it does not fit.
fn paint_elided(ui: &Ui, rect: Rect, x: f32, right: f32, text: &str, color: Color32) {
    let available = right - x;
    if available <= 8.0 {
        return;
    }
    let mut shown = text.to_string();
    if text_width(ui, text, body()) > available {
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let candidate: String = chars[..mid].iter().collect::<String>() + "…";
            if text_width(ui, &candidate, body()) <= available {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        shown = chars[..lo]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string()
            + "…";
    }
    let clip = Rect::from_min_max(rect.min, pos2(right, rect.bottom()));
    kit::paint_text(ui, clip, x, &shown, color, body());
}

fn lane_color(index: usize) -> Color32 {
    LANES[index % LANES.len()]
}

fn ref_color(t: &Theme, r: &crate::plugins::builtins::git::repo::Ref) -> Color32 {
    match r.kind {
        _ if r.current => t.warning,
        RefKind::Head => t.warning,
        RefKind::Local => t.success,
        RefKind::Remote => Color32::from_rgb(0xB0, 0x8B, 0xF0),
        RefKind::Tag => Color32::from_rgb(0xE0, 0xA3, 0x5F),
    }
}

/// Content tabs of a tool window: plain text, the active one underlined with the accent color.
fn tabs<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) {
    let t = kit::theme(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        for &(value, label) in items {
            let active = *current == value;
            let width = text_width(ui, label, body()) + 20.0;
            let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
            if response.hovered() && !active {
                ui.painter()
                    .rect_filled(rect.shrink2(vec2(0.0, 3.0)), 5.0, t.hover);
            }
            let color = if active || response.hovered() {
                t.text
            } else {
                t.text_secondary
            };
            ui.painter()
                .text(rect.center(), Align2::CENTER_CENTER, label, body(), color);
            if active {
                let line = Rect::from_min_max(
                    pos2(rect.left() + 4.0, rect.bottom() - 2.0),
                    pos2(rect.right() - 4.0, rect.bottom()),
                );
                ui.painter().rect_filled(line, 1.0, t.accent);
            }
            if response.clicked() {
                *current = value;
            }
        }
    });
}

/// Toolbar of icon buttons.
fn toolbar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        add(ui)
    })
    .inner
}

fn tool_separator(ui: &mut Ui) {
    let t = kit::theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, kit::BUTTON), Sense::hover());
    let y = rect.center().y;
    ui.painter().vline(
        rect.center().x,
        (y - 8.0)..=(y + 8.0),
        stroke(1.0, t.border),
    );
}

/// Row of a menu or popup, IntelliJ style: icon column, label, shortcut or chevron on the right.
fn menu_row(ui: &mut Ui, icon: Option<&str>, text: &str, keys: &str, submenu: bool) -> Response {
    let t = kit::theme(ui.ctx());
    let enabled = ui.is_enabled();
    let color = if enabled { t.text } else { t.text_disabled };
    let keys_w = if keys.is_empty() {
        0.0
    } else {
        text_width(ui, keys, body()) + 28.0
    };
    let desired = 32.0 + text_width(ui, text, body()) + keys_w + if submenu { 28.0 } else { 12.0 };
    let width = desired.max(ui.min_rect().width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.selection);
    }
    if let Some(icon) = icon {
        kit::paint_icon(
            ui,
            icon,
            Rect::from_min_size(
                pos2(rect.left() + 8.0, rect.center().y - 8.0),
                Vec2::splat(16.0),
            ),
        );
    }
    let text_right = rect.right() - keys_w - if submenu { 24.0 } else { 6.0 };
    kit::paint_text(
        ui,
        Rect::from_min_max(rect.min, pos2(text_right, rect.bottom())),
        rect.left() + 32.0,
        text,
        color,
        body(),
    );
    if !keys.is_empty() {
        let right = rect.right() - if submenu { 28.0 } else { 10.0 };
        ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            keys,
            body(),
            t.text_secondary,
        );
    }
    if submenu {
        kit::paint_icon(
            ui,
            "chevronRight",
            Rect::from_min_size(
                pos2(rect.right() - 22.0, rect.center().y - 8.0),
                Vec2::splat(16.0),
            ),
        );
    }
    response
}

fn menu_item(ui: &mut Ui, icon: Option<&str>, text: &str, keys: &str) -> Response {
    menu_row(ui, icon, text, keys, false)
}

/// A menu item that closes its menu when clicked.
fn item(ui: &mut Ui, icon: Option<&str>, text: &str) -> bool {
    let clicked = menu_item(ui, icon, text, "").clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn menu_separator(ui: &mut Ui) {
    let t = kit::theme(ui.ctx());
    let width = ui.min_rect().width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 9.0), Sense::hover());
    ui.painter().hline(
        (rect.left() + 6.0)..=(rect.right() - 6.0),
        rect.center().y,
        stroke(1.0, t.border),
    );
}

/// A full-width tree row; hover follows the pointer even over the row's own checkbox.
fn tree_row(ui: &mut Ui, selected: bool) -> (Rect, Response) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), kit::ROW), Sense::click());
    kit::paint_row(ui, rect, selected, response.contains_pointer());
    (rect, response)
}

/// IntelliJ checkbox drawn inside a row, on top of the row's own click area.
fn row_check(ui: &Ui, id: Id, x: f32, row: Rect, state: Option<bool>) -> Response {
    let t = kit::theme(ui.ctx());
    let boxed = Rect::from_min_size(pos2(x, row.center().y - 8.0), Vec2::splat(16.0));
    let response = ui.interact(boxed.expand(2.0), id, Sense::click());
    let painter = ui.painter();
    if state != Some(false) {
        let fill = if response.hovered() {
            t.accent_hover
        } else {
            t.accent
        };
        painter.rect_filled(boxed, 4.0, fill);
        let white = stroke(1.6, Color32::WHITE);
        let c = boxed.center();
        if state.is_none() {
            painter.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], white);
        } else {
            painter.line_segment([c + vec2(-4.0, 0.0), c + vec2(-1.2, 3.0)], white);
            painter.line_segment([c + vec2(-1.2, 3.0), c + vec2(4.2, -3.2)], white);
        }
    } else {
        let border = if response.hovered() {
            t.accent
        } else {
            t.border
        };
        painter.rect(
            boxed,
            4.0,
            t.field,
            stroke(1.0, border),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// File icon, name in the color of its status and folder in gray, as in IntelliJ change lists.
fn paint_change(ui: &Ui, rect: Rect, x: f32, change: &Change, t: &Theme) {
    let (name, dir) = file_name(&change.path);
    egui::Image::new(crate::icons::file(Path::new(name), t.dark)).paint_at(
        ui,
        Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(16.0)),
    );
    let end = kit::paint_text(
        ui,
        rect,
        x + 22.0,
        name,
        status_color(t, change.kind),
        body(),
    );
    if !dir.is_empty() {
        kit::paint_text(ui, rect, end + 8.0, dir, t.text_secondary, body());
    }
}

fn change_tooltip(change: &Change) -> String {
    match &change.orig {
        Some(orig) => format!("{orig}, renommé en {}", change.path),
        None => change.path.clone(),
    }
}

/// Small arrow drawn with lines: the font has no arrow glyphs.
fn paint_arrow(painter: &egui::Painter, center: Pos2, up: bool, color: Color32) {
    let s = stroke(1.3, color);
    let (top, bottom) = (center - vec2(0.0, 4.5), center + vec2(0.0, 4.5));
    painter.line_segment([top, bottom], s);
    let (tip, dir) = if up { (top, 1.0) } else { (bottom, -1.0) };
    painter.line_segment([tip, tip + vec2(-3.0, 3.0 * dir)], s);
    painter.line_segment([tip, tip + vec2(3.0, 3.0 * dir)], s);
}

/// Outgoing and incoming commit counts: an arrow and a number for each.
fn paint_sync(ui: &Ui, painter: &egui::Painter, x: f32, y: f32, ahead: u32, behind: u32) -> f32 {
    let t = kit::theme(ui.ctx());
    let mut x = x;
    for (up, n) in [(true, ahead), (false, behind)] {
        if n == 0 {
            continue;
        }
        paint_arrow(painter, pos2(x + 4.0, y), up, t.text_secondary);
        let galley =
            painter.layout_no_wrap(n.to_string(), FontId::proportional(12.0), t.text_secondary);
        let w = galley.size().x;
        painter.galley(
            pos2(x + 10.0, y - galley.size().y / 2.0),
            galley,
            t.text_secondary,
        );
        x += 16.0 + w;
    }
    x
}

fn sync_width(ui: &Ui, ahead: u32, behind: u32) -> f32 {
    [ahead, behind]
        .iter()
        .filter(|n| **n > 0)
        .map(|n| 16.0 + text_width(ui, &n.to_string(), FontId::proportional(12.0)))
        .sum()
}

fn field(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
    ui.add(
        TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(8, 5)),
    )
}

/// A form row: a label in a fixed column, then the control.
fn labeled<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(110.0, 28.0), Sense::hover());
        let t = kit::theme(ui.ctx());
        kit::paint_text(ui, rect, rect.left(), label, t.text, body());
        add(ui)
    })
    .inner
}

fn secondary(ui: &mut Ui, text: impl Into<String>) {
    let t = kit::theme(ui.ctx());
    ui.label(RichText::new(text.into()).color(t.text_secondary));
}

// ---- Actions collected while drawing ---------------------------------------------------------

enum ChangeAction {
    Diff(Change),
    Open(String),
    History(String),
    Rollback(Vec<Change>),
    Add(Vec<String>),
    Ignore(String),
    Resolve(String),
    Accept(String, bool),
    Resolved(String),
}

enum LogAction {
    Select(String),
    Checkout(Target),
    NewBranch(String),
    NewTag(String),
    CherryPick(String, bool),
    Revert(String, bool),
    Reset(String, String),
    Undo,
    DeleteTag(String),
    Merge(String),
    Diff(Change),
    Open(String),
    History(String),
}

impl GitPlugin {
    fn head_label(&self) -> String {
        let head = &self.snap.head;
        match (&head.branch, &head.oid) {
            (Some(branch), _) => branch.clone(),
            (None, Some(oid)) => format!("HEAD détaché {}", short(oid)),
            (None, None) => "Git".into(),
        }
    }

    fn state_label(&self) -> Option<&'static str> {
        match self.snap.state {
            State::Clean => None,
            State::Merging => Some("Fusion en cours"),
            State::Rebasing => Some("Rebase en cours"),
            State::CherryPicking => Some("Cherry-pick en cours"),
            State::Reverting => Some("Revert en cours"),
        }
    }

    fn toggle_branch_popup(&mut self, ctx: &egui::Context, anchor: Pos2, pivot: Align2) {
        if self.branch_popup.take().is_some() {
            self.branch_sub = None;
        } else {
            self.branch_filter.clear();
            self.branch_popup = Some((anchor, pivot, ctx.cumulative_pass_nr()));
        }
    }

    fn toggle_collapsed(&mut self, key: String) {
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
    }

    // ---- Title bar and status bar ---------------------------------------------------------

    pub(super) fn branch_widget(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        if self.repo.is_none() {
            if self.project.is_some() {
                let r = kit::icon_text_button(ui, Some("git"), "Contrôle de version", true);
                if r.clicked() {
                    self.toggle_branch_popup(
                        ui.ctx(),
                        r.rect.left_bottom() + vec2(0.0, 4.0),
                        Align2::LEFT_TOP,
                    );
                }
            }
            return;
        }
        let label = self.head_label();
        let r =
            kit::icon_text_button(ui, Some("branch"), &label, true).on_hover_text("Branches Git");
        let (ahead, behind) = (self.snap.head.ahead, self.snap.head.behind);
        let mut clicked = r.clicked();
        let mut anchor = r.rect;
        if ahead > 0 || behind > 0 {
            let (rect, sync) = ui.allocate_exact_size(
                vec2(sync_width(ui, ahead, behind) + 4.0, 28.0),
                Sense::click(),
            );
            paint_sync(
                ui,
                ui.painter(),
                rect.left(),
                rect.center().y,
                ahead,
                behind,
            );
            clicked |= sync
                .on_hover_text(format!(
                    "{} à pousser, {} à récupérer",
                    plural(ahead as usize, "commit", "commits"),
                    plural(behind as usize, "commit", "commits")
                ))
                .clicked();
            anchor = anchor.union(rect);
        }
        if let Some(state) = self.state_label() {
            let t = self.theme;
            ui.label(RichText::new(state).color(t.warning));
        }
        if clicked {
            self.toggle_branch_popup(
                ui.ctx(),
                anchor.left_bottom() + vec2(0.0, 4.0),
                Align2::LEFT_TOP,
            );
        }
    }

    /// Status bar widgets, laid out right to left: the branch, then the running operation.
    pub(super) fn status_widget(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let t = self.theme;
        if self.repo.is_some() {
            let label = self.head_label();
            let font = FontId::proportional(12.5);
            let width = text_width(ui, &label, font.clone()) + 34.0;
            let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
            kit::paint_hover(ui, rect, &response, self.branch_popup.is_some());
            kit::paint_icon(
                ui,
                "branch",
                Rect::from_min_size(
                    pos2(rect.left() + 6.0, rect.center().y - 8.0),
                    Vec2::splat(16.0),
                ),
            );
            ui.painter().text(
                pos2(rect.left() + 26.0, rect.center().y),
                Align2::LEFT_CENTER,
                &label,
                font,
                t.text_secondary,
            );
            if response.on_hover_text("Branche Git courante").clicked() {
                self.toggle_branch_popup(
                    ui.ctx(),
                    rect.right_top() - vec2(0.0, 4.0),
                    Align2::RIGHT_BOTTOM,
                );
            }
        }
        if let Some(job) = &self.job {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{}…", job.label))
                    .size(12.5)
                    .color(t.text_secondary),
            );
            ui.add(egui::Spinner::new().size(12.0).color(t.accent));
        }
    }

    // ---- Branch popup ---------------------------------------------------------------------

    pub(super) fn branch_popup_window(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        let Some((anchor, pivot, opened)) = self.branch_popup else {
            return;
        };
        let t = self.theme;
        let pass = ctx.cumulative_pass_nr();
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // Some(None) hides the branch actions, Some(Some) shows them for another branch.
        let mut sub: Option<Option<(Branch, Pos2)>> = None;
        const WIDTH: f32 = 360.0;
        let main = egui::Area::new(Id::new("git_branch_popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .pivot(pivot)
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .inner_margin(6)
                    .corner_radius(8)
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if self.repo.is_none() {
                            if item(ui, Some("add"), "Créer un dépôt Git") {
                                self.init(host);
                                close = true;
                            }
                            if item(ui, Some("git"), "Cloner un dépôt…") {
                                self.run_command("git.clone", host);
                                close = true;
                            }
                            return;
                        }
                        let search = kit::search_field(
                            ui,
                            &mut self.branch_filter,
                            "Rechercher une branche ou une action",
                            WIDTH,
                        );
                        if pass <= opened + 1 {
                            search.request_focus();
                        }
                        ui.add_space(6.0);
                        let filter = self.branch_filter.trim().to_lowercase();
                        let actions = [
                            ("update", "Mettre à jour le projet…", "Ctrl+T", "git.update"),
                            ("commit", "Commit…", "Ctrl+K", "git.commit"),
                            ("push", "Pousser…", "Ctrl+Shift+K", "git.push"),
                            ("fetch", "Récupérer (fetch)", "", "git.fetch"),
                            ("add", "Nouvelle branche…", "", "git.branch"),
                            (
                                "tag",
                                "Checkout d'un tag ou d'une révision…",
                                "",
                                "git.checkout",
                            ),
                            ("settings", "Gérer les remotes…", "", "git.remotes"),
                        ];
                        let mut run = None;
                        for (icon, label, keys, command) in actions {
                            if !filter.is_empty() && !label.to_lowercase().contains(&filter) {
                                continue;
                            }
                            let r = menu_item(ui, Some(icon), label, keys);
                            if r.hovered() {
                                sub = Some(None);
                            }
                            if r.clicked() {
                                run = Some(command);
                            }
                        }
                        if let Some(command) = run {
                            self.run_command(command, host);
                            close = true;
                        }
                        menu_separator(ui);
                        egui::ScrollArea::vertical()
                            .max_height(380.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.set_min_width(WIDTH);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                let shown_sub = self
                                    .branch_sub
                                    .as_ref()
                                    .map(|(b, _)| (b.name.clone(), b.remote));
                                for (title, remote) in [("Locales", false), ("Distantes", true)] {
                                    let list: Vec<Branch> = self
                                        .snap
                                        .branches
                                        .iter()
                                        .filter(|b| {
                                            b.remote == remote
                                                && b.name.to_lowercase().contains(&filter)
                                        })
                                        .cloned()
                                        .collect();
                                    if list.is_empty() {
                                        continue;
                                    }
                                    let key = format!("popup:{title}");
                                    let open = !filter.is_empty() || !self.collapsed.contains(&key);
                                    let (rect, r) =
                                        ui.allocate_exact_size(vec2(WIDTH, 26.0), Sense::click());
                                    kit::paint_chevron(
                                        ui,
                                        pos2(rect.left() + 14.0, rect.center().y),
                                        open,
                                    );
                                    kit::paint_text(
                                        ui,
                                        rect,
                                        rect.left() + 26.0,
                                        title,
                                        t.text_secondary,
                                        kit::semibold(12.0),
                                    );
                                    if r.hovered() {
                                        sub = Some(None);
                                    }
                                    if r.clicked() {
                                        self.toggle_collapsed(key);
                                    }
                                    if !open {
                                        continue;
                                    }
                                    for branch in list {
                                        let active = shown_sub.as_ref()
                                            == Some(&(branch.name.clone(), branch.remote));
                                        let r = self.popup_branch_row(ui, &branch, active, WIDTH);
                                        if r.hovered() || r.clicked() {
                                            let at =
                                                pos2(r.rect.right() + 10.0, r.rect.top() - 6.0);
                                            sub = Some(Some((branch, at)));
                                        }
                                    }
                                }
                            });
                    });
            });
        if let Some(sub) = sub {
            // Keep the position of the actions while the pointer stays on the same branch.
            let same = matches!((&sub, &self.branch_sub), (Some((a, _)), Some((b, _))) if a.name == b.name && a.remote == b.remote);
            if !same {
                self.branch_sub = sub;
            }
        }
        let mut sub_rect = None;
        if !close && let Some((branch, at)) = self.branch_sub.clone() {
            let shown = egui::Area::new(Id::new("git_branch_actions"))
                .order(egui::Order::Foreground)
                .fixed_pos(at)
                .show(ctx, |ui| {
                    Frame::popup(ui.style())
                        .inner_margin(6)
                        .corner_radius(8)
                        .show(ui, |ui| {
                            ui.set_min_width(260.0);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if branch.remote {
                                self.remote_branch_menu(ui, &branch, host)
                            } else {
                                self.local_branch_menu(ui, &branch, host)
                            }
                        })
                        .inner
                });
            close |= shown.inner;
            sub_rect = Some(shown.response.rect);
        }
        if pass > opened
            && ctx.input(|i| i.pointer.any_click())
            && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
            && !main.response.rect.contains(p)
            && !sub_rect.is_some_and(|r| r.contains(p))
        {
            close = true;
        }
        if close {
            self.branch_popup = None;
            self.branch_sub = None;
        }
    }

    fn popup_branch_row(&self, ui: &mut Ui, branch: &Branch, active: bool, width: f32) -> Response {
        let t = self.theme;
        let (rect, r) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
        if active || r.hovered() {
            ui.painter().rect_filled(rect, 4.0, t.selection);
        }
        let icon = if branch.current { "star" } else { "branch" };
        kit::paint_icon(
            ui,
            icon,
            Rect::from_min_size(
                pos2(rect.left() + 26.0, rect.center().y - 8.0),
                Vec2::splat(16.0),
            ),
        );
        let mut right = rect.right() - 26.0;
        if let Some(upstream) = &branch.upstream {
            let w = text_width(ui, upstream, body());
            right -= w;
            kit::paint_text(ui, rect, right, upstream, t.text_secondary, body());
            right -= 12.0;
        }
        let clip = Rect::from_min_max(rect.min, pos2(right, rect.bottom()));
        let end = kit::paint_text(ui, clip, rect.left() + 50.0, &branch.name, t.text, body());
        let (ahead, behind) = match branch.track.as_str() {
            ">" => (1, 0),
            "<" => (0, 1),
            "<>" => (1, 1),
            _ => (0, 0),
        };
        if ahead + behind > 0 && end + 24.0 < right {
            let painter = ui.painter();
            let mut x = end + 8.0;
            for (up, n) in [(true, ahead), (false, behind)] {
                if n > 0 {
                    paint_arrow(
                        painter,
                        pos2(x + 4.0, rect.center().y),
                        up,
                        t.text_secondary,
                    );
                    x += 10.0;
                }
            }
        }
        kit::paint_icon(
            ui,
            "chevronRight",
            Rect::from_min_size(
                pos2(rect.right() - 22.0, rect.center().y - 8.0),
                Vec2::splat(16.0),
            ),
        );
        r
    }

    /// Actions on a local branch, in the branch popup and the log's branch tree.
    /// Returns true when one was chosen.
    fn local_branch_menu(&mut self, ui: &mut Ui, branch: &Branch, host: &mut dyn Host) -> bool {
        let name = branch.name.clone();
        let current = self.current_branch().unwrap_or("HEAD").to_string();
        let mut done = false;
        if !branch.current && item(ui, None, "Checkout") {
            self.checkout(Target::Branch(name.clone()), host);
            done = true;
        }
        if item(
            ui,
            Some("add"),
            &format!("Nouvelle branche depuis « {name} »…"),
        ) {
            self.dialog = Some(Dialog::NewBranch {
                from: Some(name.clone()),
                name: String::new(),
                checkout: true,
            });
            done = true;
        }
        if branch.current {
            if item(ui, Some("update"), "Mettre à jour…") {
                self.open_update();
                done = true;
            }
            if item(ui, Some("push"), "Pousser…") {
                self.open_push(host);
                done = true;
            }
        } else {
            menu_separator(ui);
            if item(
                ui,
                Some("merge"),
                &format!("Fusionner « {name} » dans « {current} »"),
            ) {
                self.merge(name.clone(), host);
                done = true;
            }
            if item(ui, None, &format!("Rebaser « {current} » sur « {name} »")) {
                self.rebase(name.clone(), host);
                done = true;
            }
        }
        menu_separator(ui);
        if item(ui, Some("edit"), "Renommer…") {
            self.dialog = Some(Dialog::RenameBranch {
                old: name.clone(),
                name: name.clone(),
            });
            done = true;
        }
        if !branch.current && item(ui, Some("delete"), "Supprimer…") {
            self.dialog = Some(Dialog::DeleteBranch { name, force: false });
            done = true;
        }
        done
    }

    fn remote_target(&self, name: &str) -> Target {
        let local = name.split_once('/').map_or(name, |(_, b)| b);
        if self
            .snap
            .branches
            .iter()
            .any(|b| !b.remote && b.name == local)
        {
            Target::Branch(local.to_string())
        } else {
            Target::Remote(name.to_string())
        }
    }

    fn remote_branch_menu(&mut self, ui: &mut Ui, branch: &Branch, host: &mut dyn Host) -> bool {
        let name = branch.name.clone();
        let current = self.current_branch().unwrap_or("HEAD").to_string();
        let mut done = false;
        if item(ui, None, "Checkout") {
            let target = self.remote_target(&name);
            self.checkout(target, host);
            done = true;
        }
        if item(
            ui,
            Some("add"),
            &format!("Nouvelle branche depuis « {name} »…"),
        ) {
            self.dialog = Some(Dialog::NewBranch {
                from: Some(name.clone()),
                name: String::new(),
                checkout: true,
            });
            done = true;
        }
        menu_separator(ui);
        if item(
            ui,
            Some("merge"),
            &format!("Fusionner « {name} » dans « {current} »"),
        ) {
            self.merge(name.clone(), host);
            done = true;
        }
        if item(ui, None, &format!("Rebaser « {current} » sur « {name} »")) {
            self.rebase(name.clone(), host);
            done = true;
        }
        menu_separator(ui);
        if item(ui, Some("delete"), "Supprimer du serveur…") {
            self.delete_remote_branch(name);
            done = true;
        }
        done
    }

    // ---- Commit tool window -----------------------------------------------------------------

    pub(super) fn commit_window(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let t = self.theme;
        if let Some(error) = &self.error {
            ui.add_space(8.0);
            ui.label(RichText::new(error).color(t.error));
            return;
        }
        if self.repo.is_none() {
            self.no_repository(ui, host);
            return;
        }
        tabs(
            ui,
            &mut self.commit_tab,
            &[(CommitTab::Changes, "Commit"), (CommitTab::Stash, "Stash")],
        );
        match self.commit_tab {
            CommitTab::Changes => self.changes_view(ui, host),
            CommitTab::Stash => self.stash_view(ui, host),
        }
    }

    fn no_repository(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            secondary(ui, "Ce projet n'est pas sous contrôle de version.");
            ui.add_space(10.0);
            if kit::primary_button(ui, "Créer un dépôt Git").clicked() {
                self.init(host);
            }
            ui.add_space(4.0);
            if kit::button(ui, "Cloner un dépôt…").clicked() {
                self.run_command("git.clone", host);
            }
        });
    }

    fn selected_change(&self) -> Option<Change> {
        let selected = self.selected.as_ref()?;
        self.snap
            .changes
            .iter()
            .find(|c| &c.path == selected)
            .cloned()
    }

    fn changes_view(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let t = self.theme;
        let mut action = None;
        toolbar(ui, |ui| {
            if kit::icon_button(ui, "refresh", "Actualiser").clicked() {
                self.invalidate();
            }
            if kit::icon_button(ui, "rollback", "Annuler les modifications…  Ctrl+Alt+Z").clicked()
            {
                let mut changes: Vec<Change> = self
                    .checked()
                    .into_iter()
                    .filter(|c| c.kind != Kind::Untracked)
                    .collect();
                if let Some(selected) = self.selected_change()
                    && changes.is_empty()
                {
                    changes.push(selected);
                }
                action = Some(ChangeAction::Rollback(changes));
            }
            let selected = self.selected_change();
            if kit::icon_button_enabled(ui, "diff", selected.is_some(), "Afficher le diff  Ctrl+D")
                .clicked()
                && let Some(change) = selected
            {
                action = Some(ChangeAction::Diff(change));
            }
            tool_separator(ui);
            if kit::icon_button(ui, "update", "Mettre à jour le projet…  Ctrl+T").clicked() {
                self.open_update();
            }
            if kit::icon_button(ui, "push", "Pousser…  Ctrl+Shift+K").clicked() {
                self.open_push(host);
            }
            tool_separator(ui);
            if kit::icon_button(ui, "expandAll", "Tout déplier").clicked() {
                self.collapsed.retain(|k| !k.starts_with("changes:"));
            }
            if kit::icon_button(ui, "collapseAll", "Tout replier").clicked() {
                for (title, _) in GROUPS {
                    self.collapsed.insert(format!("changes:{title}"));
                }
            }
        });
        if let Some(state) = self.state_label() {
            Frame::new()
                .fill(t.warning.gamma_multiply(0.12))
                .stroke(stroke(1.0, t.warning.gamma_multiply(0.35)))
                .corner_radius(6)
                .inner_margin(Margin::symmetric(10, 8))
                .outer_margin(Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(state).color(t.text));
                    secondary(
                        ui,
                        if self.snap.state == State::Merging {
                            "Résolvez les conflits puis commitez."
                        } else {
                            "Résolvez les conflits puis continuez."
                        },
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if self.snap.state != State::Merging
                            && kit::primary_button(ui, "Continuer").clicked()
                        {
                            self.continue_operation(host);
                        }
                        if kit::button(ui, "Abandonner").clicked() {
                            self.abort_operation();
                        }
                    });
                });
        }

        egui::TopBottomPanel::bottom("git_commit_box")
            .frame(Frame::NONE.inner_margin(Margin {
                left: 8,
                right: 8,
                top: 4,
                bottom: 10,
            }))
            .show_separator_line(true)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if kit::checkbox(ui, &mut self.amend, "Modifier le dernier commit").changed() {
                        self.toggle_amend();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let r = kit::icon_toggle(ui, "settings", self.signoff, "Options du commit");
                        Popup::menu(&r)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(180.0);
                                kit::checkbox(ui, &mut self.signoff, "Sign-off");
                            });
                    });
                });
                ui.add_space(2.0);
                let edit = ui.add(
                    TextEdit::multiline(&mut self.message)
                        .hint_text("Message du commit")
                        .frame(false)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY)
                        .margin(Margin::symmetric(2, 6)),
                );
                if self.focus_message {
                    edit.request_focus();
                    self.focus_message = false;
                }
                let submit = edit.has_focus()
                    && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if self.amend {
                        "Modifier le commit"
                    } else {
                        "Commit"
                    };
                    if kit::primary_button(ui, label)
                        .on_hover_text("Ctrl+Entrée")
                        .clicked()
                        || submit
                    {
                        self.commit(host, false);
                    }
                    if kit::button(ui, "Commit et pousser…").clicked() {
                        self.commit(host, true);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.changes_tree(ui, &mut action));
                if self.loaded && self.snap.changes.is_empty() {
                    let rect = ui.max_rect();
                    kit::paint_centered(ui, rect, "Aucune modification");
                } else if !self.loaded {
                    kit::paint_centered(ui, ui.max_rect(), "Chargement…");
                }
            });
        if let Some(action) = action {
            self.change_action(action, host);
        }
    }

    fn changes_tree(&mut self, ui: &mut Ui, action: &mut Option<ChangeAction>) {
        let t = self.theme;
        ui.spacing_mut().item_spacing.y = 0.0;
        let changes = self.snap.changes.clone();
        if changes.is_empty() {
            return;
        }
        for (title, group) in GROUPS {
            let members: Vec<&Change> = changes
                .iter()
                .filter(|c| match group {
                    Kind::Modified => !matches!(c.kind, Kind::Untracked | Kind::Conflicted),
                    other => c.kind == other,
                })
                .collect();
            if members.is_empty() {
                continue;
            }
            let key = format!("changes:{title}");
            let open = !self.collapsed.contains(&key);
            let included = members.iter().filter(|c| self.included(c)).count();
            let state = match included {
                0 => Some(false),
                n if n == members.len() => Some(true),
                _ => None,
            };
            let (rect, response) = tree_row(ui, false);
            let left = rect.left() + 4.0;
            kit::paint_chevron(ui, pos2(left + 10.0, rect.center().y), open);
            let end = kit::paint_text(ui, rect, left + 46.0, title, t.text, body());
            kit::paint_text(
                ui,
                rect,
                end + 8.0,
                &plural(members.len(), "fichier", "fichiers"),
                t.text_secondary,
                body(),
            );
            if row_check(ui, Id::new(("git_group", title)), left + 22.0, rect, state).clicked() {
                for change in &members {
                    self.set_included(change, state != Some(true));
                }
            } else if response.clicked() {
                self.toggle_collapsed(key);
            }
            if !open {
                continue;
            }
            for change in members {
                let selected = self.selected.as_ref() == Some(&change.path);
                let (rect, response) = tree_row(ui, selected);
                let left = rect.left() + 4.0;
                let on = self.included(change);
                let check = row_check(
                    ui,
                    Id::new(("git_change", &change.path)),
                    left + 42.0,
                    rect,
                    Some(on),
                );
                paint_change(ui, rect, left + 64.0, change, &t);
                if check.clicked() {
                    self.set_included(change, !on);
                }
                let response = response.on_hover_text_at_pointer(change_tooltip(change));
                if response.clicked() || response.secondary_clicked() {
                    self.selected = Some(change.path.clone());
                }
                if response.double_clicked() {
                    *action = Some(if change.kind == Kind::Conflicted {
                        ChangeAction::Resolve(change.path.clone())
                    } else {
                        ChangeAction::Diff(change.clone())
                    });
                }
                response.context_menu(|ui| {
                    ui.set_min_width(240.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if let Some(a) = change_menu(ui, change) {
                        *action = Some(a);
                    }
                });
            }
        }
    }

    fn change_action(&mut self, action: ChangeAction, host: &mut dyn Host) {
        let Some(repo) = &self.repo else { return };
        match action {
            ChangeAction::Diff(change) => self.open_diff(DiffSource::WorkingTree(change)),
            ChangeAction::Open(path) => host.open_file(repo.abs(&path)),
            ChangeAction::History(path) => {
                let path = repo.abs(&path);
                self.show_history(&path, host);
            }
            ChangeAction::Rollback(changes) => {
                if host.save_files() {
                    self.rollback(changes);
                }
            }
            ChangeAction::Add(paths) => self.add_to_vcs(paths),
            ChangeAction::Ignore(path) => self.ignore(path),
            ChangeAction::Resolve(path) => self.open_resolver(path),
            ChangeAction::Accept(path, theirs) => self.accept(path, theirs),
            ChangeAction::Resolved(path) => self.mark_resolved(path),
        }
    }

    fn stash_view(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let t = self.theme;
        let mut command: Option<(&str, String)> = None;
        let selected = self.stash_selected.clone();
        toolbar(ui, |ui| {
            if kit::icon_button(ui, "refresh", "Actualiser").clicked() {
                self.invalidate();
            }
            if kit::icon_button(ui, "shelve", "Mettre de côté…").clicked() {
                self.run_command("git.stash", host);
            }
            tool_separator(ui);
            let enabled = selected.is_some();
            if kit::icon_button_enabled(ui, "unshelve", enabled, "Appliquer").clicked()
                && let Some(name) = &selected
            {
                command = Some(("apply", name.clone()));
            }
            if kit::icon_button_enabled(ui, "delete", enabled, "Supprimer…").clicked()
                && let Some(name) = &selected
            {
                command = Some(("drop", name.clone()));
            }
        });
        let mut select = None;
        let mut diff = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for stash in &self.snap.stashes {
                    let open = selected.as_ref() == Some(&stash.name);
                    let (rect, response) = tree_row(ui, open);
                    let left = rect.left() + 4.0;
                    kit::paint_chevron(ui, pos2(left + 10.0, rect.center().y), open);
                    kit::paint_icon(
                        ui,
                        "shelve",
                        Rect::from_min_size(
                            pos2(left + 22.0, rect.center().y - 8.0),
                            Vec2::splat(16.0),
                        ),
                    );
                    let end =
                        kit::paint_text(ui, rect, left + 44.0, &stash.message, t.text, body());
                    kit::paint_text(ui, rect, end + 8.0, &stash.name, t.text_secondary, body());
                    if response.clicked() || response.secondary_clicked() {
                        select = Some(stash.name.clone());
                    }
                    response.context_menu(|ui| {
                        ui.set_min_width(200.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (icon, label, verb) in [
                            (Some("unshelve"), "Appliquer", "apply"),
                            (None, "Restaurer (pop)", "pop"),
                            (Some("delete"), "Supprimer…", "drop"),
                        ] {
                            if item(ui, icon, label) {
                                command = Some((verb, stash.name.clone()));
                            }
                        }
                    });
                    if open {
                        for change in &self.stash_files {
                            let (rect, response) = tree_row(ui, false);
                            paint_change(ui, rect, rect.left() + 48.0, change, &t);
                            if response
                                .on_hover_text_at_pointer(change_tooltip(change))
                                .double_clicked()
                            {
                                diff = Some(change.clone());
                            }
                        }
                    }
                }
                if self.snap.stashes.is_empty() {
                    kit::paint_centered(ui, ui.max_rect(), "Aucune modification mise de côté");
                }
            });
        if let (Some(change), Some(name)) = (diff, selected) {
            self.open_diff(DiffSource::Revision {
                from: Some(format!("{name}^1")),
                to: name,
                change,
            });
        }
        if let Some(name) = select {
            if self.stash_selected.as_ref() == Some(&name) {
                self.stash_selected = None;
                self.stash_files.clear();
            } else {
                self.select_stash(name);
            }
        }
        match command {
            Some(("drop", name)) => {
                let label = name.clone();
                let pending = self.pending("Suppression du stash", After::Nothing, move |git| {
                    git.run(&["stash", "drop", &name])
                });
                self.confirm(
                    "Supprimer le stash",
                    format!("{label} sera définitivement supprimé."),
                    Vec::new(),
                    pending,
                );
            }
            Some((verb, name)) if host.save_files() => {
                let label = if verb == "pop" {
                    "Restauration du stash"
                } else {
                    "Application du stash"
                };
                self.git(label, &["stash", verb, &name]);
            }
            _ => {}
        }
    }

    // ---- Git tool window --------------------------------------------------------------------

    pub(super) fn log_window(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        tabs(
            ui,
            &mut self.log_tab,
            &[(LogTab::Log, "Log"), (LogTab::Console, "Console")],
        );
        match self.log_tab {
            LogTab::Console => self.console_view(ui),
            LogTab::Log if self.repo.is_none() => self.no_repository(ui, host),
            LogTab::Log => self.log_view(ui, host),
        }
    }

    fn console_view(&mut self, ui: &mut Ui) {
        let t = self.theme;
        egui::SidePanel::left("git_console_tools")
            .exact_width(34.0)
            .resizable(false)
            .frame(Frame::NONE.inner_margin(Margin::symmetric(4, 2)))
            .show_inside(ui, |ui| {
                if kit::icon_button(ui, "delete", "Effacer").clicked()
                    && let Ok(mut console) = self.console.lock()
                {
                    console.clear();
                }
            });
        egui::CentralPanel::default()
            .frame(Frame::NONE.inner_margin(Margin::symmetric(8, 4)))
            .show_inside(ui, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let Ok(console) = self.console.lock() else {
                            return;
                        };
                        if console.is_empty() {
                            kit::paint_centered(
                                ui,
                                ui.max_rect(),
                                "Les commandes Git lancées par l'IDE s'affichent ici.",
                            );
                        }
                        let font = FontId::monospace(13.0);
                        for entry in console.iter() {
                            ui.label(
                                RichText::new(&entry.command)
                                    .font(font.clone())
                                    .color(t.accent_hover),
                            );
                            if !entry.output.is_empty() {
                                let color = if entry.ok { t.text } else { t.error };
                                ui.label(
                                    RichText::new(&entry.output).font(font.clone()).color(color),
                                );
                            }
                            ui.add_space(4.0);
                        }
                    });
            });
    }

    fn parent_of(&self, hash: &str) -> Option<String> {
        self.log
            .rows
            .iter()
            .find(|c| c.hash == hash)
            .and_then(|c| c.parents.first().cloned())
    }

    fn log_view(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.log.stale {
            self.load_log();
        }
        let mut action = None;
        let full = ui.available_width();
        egui::SidePanel::left("git_log_branches")
            .resizable(true)
            .default_width((full * 0.17).clamp(160.0, 260.0))
            .width_range(120.0..=420.0)
            .frame(Frame::NONE.inner_margin(Margin {
                left: 0,
                right: 4,
                top: 0,
                bottom: 0,
            }))
            .show_inside(ui, |ui| self.branches_view(ui, host, &mut action));
        egui::SidePanel::right("git_log_details")
            .resizable(true)
            .default_width((full * 0.28).clamp(240.0, 420.0))
            .width_range(200.0..=720.0)
            .frame(Frame::NONE.inner_margin(Margin {
                left: 4,
                right: 0,
                top: 0,
                bottom: 0,
            }))
            .show_inside(ui, |ui| self.details_view(ui, &mut action));
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show_inside(ui, |ui| {
                self.log_filters(ui);
                if let Some(error) = &self.log.error {
                    ui.label(RichText::new(error).color(self.theme.error));
                }
                self.commits_view(ui, &mut action);
            });
        if let Some(action) = action {
            self.log_action(action, host);
        }
    }

    fn branches_view(&mut self, ui: &mut Ui, host: &mut dyn Host, action: &mut Option<LogAction>) {
        let t = self.theme;
        toolbar(ui, |ui| {
            if kit::icon_button(ui, "add", "Nouvelle branche…").clicked() {
                self.run_command("git.branch", host);
            }
            if kit::icon_button(ui, "fetch", "Récupérer (fetch)").clicked() {
                self.fetch();
            }
            if kit::icon_button(ui, "update", "Mettre à jour le projet…  Ctrl+T").clicked() {
                self.open_update();
            }
        });
        let filtered = self.log.query.branch.clone();
        let mut filter_to: Option<Option<String>> = None;
        let toggle = |name: &str| (filtered.as_deref() != Some(name)).then(|| name.to_string());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (rect, response) = tree_row(ui, filtered.as_deref() == Some("HEAD"));
                kit::paint_text(
                    ui,
                    rect,
                    rect.left() + 26.0,
                    "HEAD (branche courante)",
                    t.text,
                    body(),
                );
                if response.clicked() {
                    filter_to = Some(toggle("HEAD"));
                }
                let branches = self.snap.branches.clone();
                for (title, remote) in [("Locales", false), ("Distantes", true)] {
                    let list: Vec<&Branch> =
                        branches.iter().filter(|b| b.remote == remote).collect();
                    if list.is_empty() {
                        continue;
                    }
                    let key = format!("log:{title}");
                    let open = !self.collapsed.contains(&key);
                    let (rect, response) = tree_row(ui, false);
                    kit::paint_chevron(ui, pos2(rect.left() + 14.0, rect.center().y), open);
                    kit::paint_text(ui, rect, rect.left() + 26.0, title, t.text, body());
                    if response.clicked() {
                        self.toggle_collapsed(key);
                    }
                    if !open {
                        continue;
                    }
                    let mut remote_open = true;
                    let mut last_remote = "";
                    for branch in list {
                        let mut indent = 26.0;
                        let mut shown = branch.name.as_str();
                        if remote {
                            let (name, rest) = branch.name.split_once('/').unwrap_or(("", shown));
                            if name != last_remote {
                                last_remote = name;
                                let key = format!("log:remote:{name}");
                                remote_open = !self.collapsed.contains(&key);
                                let (rect, response) = tree_row(ui, false);
                                kit::paint_chevron(
                                    ui,
                                    pos2(rect.left() + 34.0, rect.center().y),
                                    remote_open,
                                );
                                kit::paint_text(ui, rect, rect.left() + 46.0, name, t.text, body());
                                if response.clicked() {
                                    self.toggle_collapsed(key);
                                }
                            }
                            if !remote_open {
                                continue;
                            }
                            indent = 46.0;
                            shown = rest;
                        }
                        let selected = filtered.as_deref() == Some(branch.name.as_str());
                        let (rect, response) = tree_row(ui, selected);
                        let icon = if branch.current { "star" } else { "branch" };
                        kit::paint_icon(
                            ui,
                            icon,
                            Rect::from_min_size(
                                pos2(rect.left() + indent, rect.center().y - 8.0),
                                Vec2::splat(16.0),
                            ),
                        );
                        kit::paint_text(
                            ui,
                            rect,
                            rect.left() + indent + 22.0,
                            shown,
                            t.text,
                            body(),
                        );
                        let response = match &branch.upstream {
                            Some(upstream) => {
                                response.on_hover_text_at_pointer(format!("Suit {upstream}"))
                            }
                            None => response,
                        };
                        if response.clicked() {
                            filter_to = Some(toggle(&branch.name));
                        }
                        if response.double_clicked() {
                            *action = Some(LogAction::Checkout(if remote {
                                self.remote_target(&branch.name)
                            } else {
                                Target::Branch(branch.name.clone())
                            }));
                        }
                        response.context_menu(|ui| {
                            ui.set_min_width(260.0);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if remote {
                                self.remote_branch_menu(ui, branch, host);
                            } else {
                                self.local_branch_menu(ui, branch, host);
                            }
                        });
                    }
                }
            });
        if let Some(branch) = filter_to {
            self.log.query.branch = branch;
            self.log.stale = true;
        }
    }

    fn log_filters(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(4.0);
            let width = (ui.available_width() * 0.3).clamp(120.0, 240.0);
            kit::search_field(ui, &mut self.log.filter, "Texte ou hash", width);
            let label = match self.log.query.branch.as_deref() {
                Some(branch) => format!("Branche : {branch}"),
                None => "Branche".to_string(),
            };
            let r = kit::icon_text_button(ui, None, &label, true);
            let mut branch: Option<Option<String>> = None;
            Popup::menu(&r).show(|ui| {
                ui.set_min_width(200.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                if menu_item(ui, None, "Toutes les branches", "").clicked() {
                    branch = Some(None);
                }
                if menu_item(ui, None, "HEAD", "").clicked() {
                    branch = Some(Some("HEAD".into()));
                }
                menu_separator(ui);
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for b in &self.snap.branches {
                            let icon = if b.current { "star" } else { "branch" };
                            if menu_item(ui, Some(icon), &b.name, "").clicked() {
                                branch = Some(Some(b.name.clone()));
                            }
                        }
                    });
            });
            if let Some(branch) = branch
                && branch != self.log.query.branch
            {
                self.log.query.branch = branch;
                self.log.stale = true;
            }
            let label = match &self.log.author {
                Some(author) => format!("Auteur : {author}"),
                None => "Auteur".to_string(),
            };
            let r = kit::icon_text_button(ui, None, &label, true);
            Popup::menu(&r).show(|ui| {
                ui.set_min_width(200.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                if menu_item(ui, None, "Tous les auteurs", "").clicked() {
                    self.log.author = None;
                }
                menu_separator(ui);
                let authors: BTreeSet<&str> =
                    self.log.rows.iter().map(|c| c.author.as_str()).collect();
                let mut chosen = None;
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for author in authors.iter().take(200) {
                            if menu_item(ui, Some("user"), author, "").clicked() {
                                chosen = Some(author.to_string());
                            }
                        }
                    });
                if chosen.is_some() {
                    self.log.author = chosen;
                }
            });
            if let Some(path) = self.log.query.path.clone() {
                let t = self.theme;
                ui.label(RichText::new(format!("Fichier : {path}")).color(t.link));
                if kit::icon_button(ui, "closeSmall", "Tout l'historique").clicked() {
                    self.log.query.path = None;
                    self.log.stale = true;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if kit::icon_button(ui, "refresh", "Actualiser").clicked() {
                    self.log.stale = true;
                }
                if self.log.loading.is_some() {
                    ui.add(egui::Spinner::new().size(14.0));
                }
            });
        });
    }

    fn commits_view(&self, ui: &mut Ui, action: &mut Option<LogAction>) {
        let filter = self.log.filter.trim().to_lowercase();
        let author = self.log.author.as_deref();
        let graph =
            filter.is_empty() && author.is_none() && self.log.graph.len() == self.log.rows.len();
        let rows: Vec<usize> = (0..self.log.rows.len())
            .filter(|&i| {
                let c = &self.log.rows[i];
                author.is_none_or(|a| c.author == a)
                    && (filter.is_empty()
                        || c.subject.to_lowercase().contains(&filter)
                        || c.hash.starts_with(&filter)
                        || c.author.to_lowercase().contains(&filter))
            })
            .collect();
        if rows.is_empty() {
            let text = if self.log.loading.is_some() {
                "Chargement…"
            } else {
                "Aucun commit"
            };
            kit::paint_centered(ui, ui.available_rect_before_wrap(), text);
            return;
        }
        let head = self.snap.head.oid.as_deref();
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, kit::ROW, rows.len(), |ui, range| {
                for &index in &rows[range] {
                    let commit = &self.log.rows[index];
                    let (rect, response) = ui
                        .allocate_exact_size(vec2(ui.available_width(), kit::ROW), Sense::click());
                    let selected = self.log.selected.as_deref() == Some(commit.hash.as_str());
                    kit::paint_row(ui, rect, selected, response.hovered());
                    let is_head = head == Some(commit.hash.as_str());
                    let lanes = if graph {
                        let row = &self.log.graph[index];
                        paint_graph(ui.painter(), rect, row, is_head, &self.theme);
                        row.width
                    } else {
                        let single = graph::GraphRow {
                            width: 1,
                            ..Default::default()
                        };
                        paint_graph(ui.painter(), rect, &single, is_head, &self.theme);
                        1
                    };
                    let x = rect.left() + 8.0 + lanes as f32 * LANE_W + 6.0;
                    self.paint_commit(ui, rect, x, commit);
                    if response.clicked() || response.secondary_clicked() {
                        *action = Some(LogAction::Select(commit.hash.clone()));
                    }
                    response.context_menu(|ui| {
                        ui.set_min_width(260.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if let Some(a) = commit_menu(ui, commit, head, self.current_branch()) {
                            *action = Some(a);
                        }
                    });
                }
            });
    }

    fn paint_commit(&self, ui: &Ui, rect: Rect, x: f32, commit: &Commit) {
        let t = &self.theme;
        let date_w = 128.0;
        let author_w = if rect.width() > 560.0 { 150.0 } else { 0.0 };
        let date_x = rect.right() - date_w;
        let author_x = date_x - author_w;
        let subject_right = author_x - 12.0;
        let badge_font = FontId::proportional(11.5);
        // Like IntelliJ, labels take at most part of the column; the others become "+N".
        let budget = ((subject_right - x) * 0.4).max(0.0);
        let mut labels: Vec<(String, Color32, f32)> = Vec::new();
        let mut used = 0.0;
        for (i, r) in commit.refs.iter().enumerate() {
            let mut name = elide_chars(&r.name, 28);
            let mut w = text_width(ui, &name, badge_font.clone()) + 14.0;
            // The first label always shows, shortened to the space left for labels.
            let mut max = name.chars().count();
            while labels.is_empty() && w > budget && max > 6 {
                max -= 1;
                name = elide_chars(&r.name, max);
                w = text_width(ui, &name, badge_font.clone()) + 14.0;
            }
            let rest = commit.refs.len() - i - 1;
            let more_w = if rest > 0 { 34.0 } else { 0.0 };
            if used + w + more_w > budget && !labels.is_empty() || used + w > budget {
                let more = format!("+{}", commit.refs.len() - i);
                let w = text_width(ui, &more, badge_font.clone()) + 14.0;
                labels.push((more, t.text_secondary, w));
                used += w;
                break;
            }
            labels.push((name, ref_color(t, r), w));
            used += w;
        }
        let badges_x = subject_right - used;
        let subject_right = if labels.is_empty() {
            subject_right
        } else {
            badges_x - 8.0
        };
        paint_elided(ui, rect, x, subject_right, &commit.subject, t.text);
        let badges = ui.painter().with_clip_rect(Rect::from_min_max(
            pos2(badges_x, rect.top()),
            pos2(badges_x + used, rect.bottom()),
        ));
        let mut bx = badges_x;
        for (name, color, w) in &labels {
            kit::paint_badge(ui, &badges, bx, rect.center().y, name, *color);
            bx += w;
        }
        if author_w > 0.0 {
            let clip = Rect::from_min_max(
                pos2(author_x, rect.top()),
                pos2(date_x - 10.0, rect.bottom()),
            );
            paint_elided(
                ui,
                clip,
                author_x,
                clip.right(),
                &commit.author,
                t.text_secondary,
            );
        }
        kit::paint_text(ui, rect, date_x, &commit.date, t.text_secondary, body());
    }

    fn details_view(&self, ui: &mut Ui, action: &mut Option<LogAction>) {
        let t = self.theme;
        let Some((hash, message, files)) = &self.log.details else {
            kit::paint_centered(ui, ui.max_rect(), "Aucun commit sélectionné");
            return;
        };
        let commit = self.log.rows.iter().find(|c| &c.hash == hash);
        let full = ui.available_height();
        egui::TopBottomPanel::bottom("git_log_meta")
            .resizable(true)
            .default_height(full * 0.5)
            .height_range(60.0..=(full - 40.0).max(61.0))
            .frame(Frame::NONE.inner_margin(Margin {
                left: 10,
                right: 8,
                top: 8,
                bottom: 6,
            }))
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(message).color(t.text)).wrap());
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(short(hash)).color(t.text_secondary));
                            if kit::icon_button(ui, "copy", "Copier le hash").clicked() {
                                ui.ctx().copy_text(hash.clone());
                            }
                            if let Some(c) = commit {
                                secondary(ui, format!("{} <{}>", c.author, c.email));
                            }
                        });
                        if let Some(c) = commit {
                            secondary(ui, format!("le {}", c.date.replace(' ', " à ")));
                            if !c.refs.is_empty() {
                                ui.add_space(6.0);
                                ui.horizontal_wrapped(|ui| {
                                    let font = FontId::proportional(11.5);
                                    for r in &c.refs {
                                        let w = text_width(ui, &r.name, font.clone()) + 10.0;
                                        let (rect, _) =
                                            ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
                                        kit::paint_badge(
                                            ui,
                                            ui.painter(),
                                            rect.left(),
                                            rect.center().y,
                                            &r.name,
                                            ref_color(&t, r),
                                        );
                                    }
                                });
                            }
                        }
                    });
            });
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let (rect, _) = tree_row(ui, false);
                        kit::paint_chevron(ui, pos2(rect.left() + 14.0, rect.center().y), true);
                        egui::Image::new(crate::icons::folder(true, crate::ui::theme(ui.ctx()).dark)).paint_at(
                            ui,
                            Rect::from_min_size(
                                pos2(rect.left() + 26.0, rect.center().y - 8.0),
                                Vec2::splat(16.0),
                            ),
                        );
                        let root = self
                            .repo
                            .as_ref()
                            .and_then(|r| r.root.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let end =
                            kit::paint_text(ui, rect, rect.left() + 48.0, &root, t.text, body());
                        kit::paint_text(
                            ui,
                            rect,
                            end + 8.0,
                            &plural(files.len(), "fichier", "fichiers"),
                            t.text_secondary,
                            body(),
                        );
                        for change in files {
                            let (rect, response) = tree_row(ui, false);
                            paint_change(ui, rect, rect.left() + 46.0, change, &t);
                            let response =
                                response.on_hover_text_at_pointer(change_tooltip(change));
                            if response.double_clicked() {
                                *action = Some(LogAction::Diff(change.clone()));
                            }
                            response.context_menu(|ui| {
                                ui.set_min_width(220.0);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if item(ui, Some("diff"), "Afficher le diff") {
                                    *action = Some(LogAction::Diff(change.clone()));
                                }
                                if change.kind != Kind::Deleted
                                    && item(ui, None, "Ouvrir la copie locale")
                                {
                                    *action = Some(LogAction::Open(change.path.clone()));
                                }
                                if item(ui, Some("history"), "Historique du fichier") {
                                    *action = Some(LogAction::History(change.path.clone()));
                                }
                            });
                        }
                    });
            });
    }

    fn log_action(&mut self, action: LogAction, host: &mut dyn Host) {
        match action {
            LogAction::Select(hash) => self.select_commit(hash),
            LogAction::Checkout(target) => self.checkout(target, host),
            LogAction::NewBranch(from) => {
                self.dialog = Some(Dialog::NewBranch {
                    from: Some(from),
                    name: String::new(),
                    checkout: true,
                })
            }
            LogAction::NewTag(rev) => {
                self.dialog = Some(Dialog::NewTag {
                    rev,
                    name: String::new(),
                    message: String::new(),
                })
            }
            LogAction::CherryPick(hash, merge) => self.cherry_pick(hash, merge, host),
            LogAction::Revert(hash, merge) => self.revert(hash, merge, host),
            LogAction::Reset(rev, subject) => {
                self.dialog = Some(Dialog::Reset {
                    rev,
                    subject,
                    mode: ResetMode::Mixed,
                })
            }
            LogAction::Undo => self.undo_commit(),
            LogAction::DeleteTag(tag) => {
                let name = tag.clone();
                let pending = self.pending("Suppression du tag", After::Nothing, move |git| {
                    git.run(&["tag", "-d", &name])
                });
                self.confirm(
                    "Supprimer le tag",
                    format!("Le tag local {tag} sera supprimé."),
                    Vec::new(),
                    pending,
                );
            }
            LogAction::Merge(name) => self.merge(name, host),
            LogAction::Diff(change) => {
                if let Some(hash) = self.log.selected.clone() {
                    let from = self.parent_of(&hash);
                    self.open_diff(DiffSource::Revision {
                        from,
                        to: hash,
                        change,
                    });
                }
            }
            LogAction::Open(path) => {
                if let Some(repo) = &self.repo {
                    host.open_file(repo.abs(&path));
                }
            }
            LogAction::History(path) => {
                if let Some(repo) = &self.repo {
                    let path = repo.abs(&path);
                    self.show_history(&path, host);
                }
            }
        }
    }

    // ---- Diff viewer ------------------------------------------------------------------------

    pub(super) fn diff_window(&mut self, ctx: &egui::Context) {
        let t = self.theme;
        let Some(view) = &mut self.diff else { return };
        let mut open = true;
        egui::Window::new(&view.path)
            .id(Id::new("git_diff"))
            .open(&mut open)
            .default_size([1100.0, 680.0])
            .collapsible(false)
            .frame(Frame::window(&ctx.style()).inner_margin(Margin::symmetric(8, 6)))
            .show(ctx, |ui| {
                let mut rebuild = false;
                toolbar(ui, |ui| {
                    if kit::icon_button(ui, "up", "Différence précédente  Maj+F7").clicked() {
                        view.step(false);
                    }
                    if kit::icon_button(ui, "down", "Différence suivante  F7").clicked() {
                        view.step(true);
                    }
                    let n = view.starts.len();
                    ui.add_space(4.0);
                    secondary(
                        ui,
                        if n == 0 {
                            "Aucune différence".to_string()
                        } else {
                            format!(
                                "{}/{}",
                                view.current + 1,
                                plural(n, "différence", "différences")
                            )
                        },
                    );
                    tool_separator(ui);
                    if kit::icon_toggle(ui, "sideBySide", view.side_by_side, "Côte à côte")
                        .clicked()
                        && !view.side_by_side
                    {
                        view.side_by_side = true;
                        rebuild = true;
                    }
                    if kit::icon_toggle(ui, "unified", !view.side_by_side, "Unifié").clicked()
                        && view.side_by_side
                    {
                        view.side_by_side = false;
                        rebuild = true;
                    }
                    tool_separator(ui);
                    rebuild |=
                        kit::checkbox(ui, &mut view.collapse, "Masquer les lignes identiques")
                            .changed();
                });
                if rebuild {
                    view.rebuild();
                }
                ui.add_space(2.0);
                if view.binary {
                    kit::paint_centered(
                        ui,
                        ui.available_rect_before_wrap(),
                        "Fichier binaire : contenu non comparable.",
                    );
                    return;
                }
                let (header, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                ui.painter().rect_filled(header, 4.0, t.frame);
                if view.side_by_side {
                    let half = header.width() / 2.0;
                    let left = Rect::from_min_size(header.min, vec2(half, header.height()));
                    let right = Rect::from_min_size(
                        pos2(header.left() + half, header.top()),
                        vec2(half, header.height()),
                    );
                    for (rect, title) in [(left, &view.left_title), (right, &view.right_title)] {
                        let end =
                            kit::paint_text(ui, rect, rect.left() + 10.0, title, t.text, body());
                        kit::paint_text(ui, rect, end + 8.0, &view.path, t.text_secondary, body());
                    }
                } else {
                    let end = kit::paint_text(
                        ui,
                        header,
                        header.left() + 10.0,
                        &format!("{}  /  {}", view.left_title, view.right_title),
                        t.text,
                        body(),
                    );
                    kit::paint_text(ui, header, end + 8.0, &view.path, t.text_secondary, body());
                }
                let font = FontId::monospace(13.0);
                let row_height = ui.fonts_mut(|f| f.row_height(&font)) + 4.0;
                let char_w = ui.fonts_mut(|f| f.glyph_width(&font, ' '));
                let style = DiffStyle { t, font, char_w };
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if let Some(index) = view.scroll_to.take() {
                    area = area.vertical_scroll_offset((index as f32 - 3.0).max(0.0) * row_height);
                }
                let mut expand = false;
                area.show_rows(ui, row_height, view.rows.len(), |ui, range| {
                    for row in &view.rows[range] {
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), row_height),
                            Sense::click(),
                        );
                        let painter = ui.painter_at(rect);
                        if row.kind == RowKind::Fold {
                            painter.rect_filled(rect, 0.0, t.diff_empty);
                            let color = if response.hovered() {
                                t.link
                            } else {
                                t.text_secondary
                            };
                            painter.text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                format!(
                                    "{} masquées : cliquez pour tout afficher",
                                    plural(row.hidden, "ligne identique", "lignes identiques")
                                ),
                                FontId::proportional(12.0),
                                color,
                            );
                            expand |= response.clicked();
                        } else if view.side_by_side {
                            paint_side_by_side(&painter, rect, row, &style);
                        } else {
                            paint_unified(&painter, rect, row, &style);
                        }
                    }
                });
                if expand {
                    view.collapse = false;
                    view.rebuild();
                }
            });
        if !open {
            self.diff = None;
        }
    }

    // ---- Conflict resolver ------------------------------------------------------------------

    pub(super) fn resolver_window(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        let t = self.theme;
        let Some(resolver) = &mut self.resolver else {
            return;
        };
        let mut open = true;
        let (mut apply, mut edit, mut cancel) = (false, false, false);
        let total = conflict::conflicts(&resolver.segments);
        egui::Window::new(format!("Fusionner les révisions de {}", resolver.path))
            .id(Id::new("git_resolver"))
            .open(&mut open)
            .default_size([1000.0, 640.0])
            .collapsible(false)
            .frame(Frame::window(&ctx.style()).inner_margin(Margin::same(10)))
            .show(ctx, |ui| {
                let remaining = resolver
                    .segments
                    .iter()
                    .filter(|s| matches!(s, Segment::Conflict(c) if c.choice.is_none()))
                    .count();
                toolbar(ui, |ui| {
                    for (icon, label, choice) in [
                        ("arrowRight", "Tout accepter à gauche (local)", Choice::Ours),
                        (
                            "arrowLeft",
                            "Tout accepter à droite (entrant)",
                            Choice::Theirs,
                        ),
                    ] {
                        if kit::icon_text_button(ui, Some(icon), label, false).clicked() {
                            for segment in &mut resolver.segments {
                                if let Segment::Conflict(c) = segment {
                                    c.choice = Some(choice);
                                }
                            }
                        }
                    }
                    tool_separator(ui);
                    if kit::icon_button(ui, "magicResolve", "Réinitialiser les choix").clicked() {
                        for segment in &mut resolver.segments {
                            if let Segment::Conflict(c) = segment {
                                c.choice = None;
                            }
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let color = if remaining == 0 {
                            t.success
                        } else {
                            t.text_secondary
                        };
                        ui.label(
                            RichText::new(if remaining == 0 {
                                "Tous les conflits sont résolus".to_string()
                            } else {
                                format!(
                                    "{remaining} sur {}",
                                    plural(total, "conflit à résoudre", "conflits à résoudre")
                                )
                            })
                            .color(color),
                        );
                    });
                });
                egui::TopBottomPanel::bottom("git_resolver_actions")
                    .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 6)))
                    .show_separator_line(false)
                    .show_inside(ui, |ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            cancel = kit::button(ui, "Annuler").clicked();
                            let label = if remaining == 0 {
                                "Appliquer et marquer comme résolu"
                            } else {
                                "Appliquer"
                            };
                            apply = kit::primary_button(ui, label).clicked();
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                edit = kit::button(ui, "Ouvrir dans l'éditeur").clicked();
                            });
                        });
                    });
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let mut number = 0;
                                for segment in &mut resolver.segments {
                                    let Segment::Conflict(c) = segment else {
                                        continue;
                                    };
                                    number += 1;
                                    conflict_block(ui, &t, number, c);
                                }
                            });
                    });
            });
        if edit && let Some(repo) = &self.repo {
            host.open_file(repo.abs(&resolver.path));
            open = false;
        }
        if apply {
            self.apply_resolution(host);
        } else if !open || cancel {
            self.resolver = None;
        }
    }

    // ---- Dialogs ----------------------------------------------------------------------------

    pub(super) fn dialog_window(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let t = self.theme;
        let mut ok = false;
        let mut cancel = false;
        let mut force = false;
        let mut remove_remote = None;
        let modal = egui::Modal::new(Id::new("git_dialog"))
            .frame(Frame::popup(&ctx.style()).inner_margin(20).corner_radius(10))
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                match &mut dialog {
                    Dialog::Confirm {
                        title, text, items, ..
                    } => {
                        kit::dialog_title(ui, title);
                        ui.label(text.as_str());
                        if !items.is_empty() {
                            Frame::new()
                                .stroke(stroke(1.0, t.border))
                                .corner_radius(6)
                                .inner_margin(4)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                        ui.spacing_mut().item_spacing.y = 0.0;
                                        for path in items.iter() {
                                            let (rect, _) = ui.allocate_exact_size(
                                                vec2(ui.available_width(), kit::ROW),
                                                Sense::hover(),
                                            );
                                            let change = self
                                                .snap
                                                .changes
                                                .iter()
                                                .find(|c| &c.path == path)
                                                .cloned()
                                                .unwrap_or(Change {
                                                    path: path.clone(),
                                                    orig: None,
                                                    kind: Kind::Modified,
                                                });
                                            paint_change(ui, rect, rect.left() + 6.0, &change, &t);
                                        }
                                    });
                                });
                        }
                        (ok, cancel) = kit::dialog_buttons(ui, "Confirmer");
                    }
                    Dialog::NewBranch {
                        from,
                        name,
                        checkout,
                    } => {
                        kit::dialog_title(ui, "Nouvelle branche");
                        if let Some(from) = from {
                            secondary(ui, format!("Depuis {}", from.get(..40).unwrap_or(from)));
                        }
                        labeled(ui, "Nom :", |ui| field(ui, name, "nom-de-branche")).request_focus();
                        kit::checkbox(ui, checkout, "Basculer sur la branche");
                        (ok, cancel) = kit::dialog_buttons(ui, "Créer");
                    }
                    Dialog::RenameBranch { old, name } => {
                        kit::dialog_title(ui, &format!("Renommer la branche « {old} »"));
                        labeled(ui, "Nouveau nom :", |ui| field(ui, name, "")).request_focus();
                        (ok, cancel) = kit::dialog_buttons(ui, "Renommer");
                    }
                    Dialog::DeleteBranch { name, force } => {
                        kit::dialog_title(ui, &format!("Supprimer la branche « {name} »"));
                        kit::checkbox(ui, force, "Forcer même si elle n'est pas fusionnée (-D)");
                        (ok, cancel) = kit::dialog_buttons(ui, "Supprimer");
                    }
                    Dialog::NewTag { rev, name, message } => {
                        kit::dialog_title(ui, "Nouveau tag");
                        secondary(ui, format!("Sur le commit {}", short(rev)));
                        labeled(ui, "Nom :", |ui| field(ui, name, "v1.0.0")).request_focus();
                        labeled(ui, "Message :", |ui| field(ui, message, "Tag annoté (optionnel)"));
                        (ok, cancel) = kit::dialog_buttons(ui, "Créer");
                    }
                    Dialog::Revision { rev } => {
                        kit::dialog_title(ui, "Checkout d'un tag ou d'une révision");
                        labeled(ui, "Révision :", |ui| field(ui, rev, "v1.0.0, a1b2c3d, HEAD~2…"))
                            .request_focus();
                        secondary(ui, "La copie de travail passera en HEAD détaché.");
                        (ok, cancel) = kit::dialog_buttons(ui, "Checkout");
                    }
                    Dialog::SmartCheckout { target } => {
                        let name = match target {
                            Target::Branch(n) | Target::Remote(n) | Target::Revision(n) => n.clone(),
                        };
                        kit::dialog_title(ui, &format!("Checkout de « {name} »"));
                        ui.add(
                            egui::Label::new(
                                "Vous avez des modifications locales qui seraient écrasées. Le checkout intelligent les met de côté, change de branche puis les restaure.",
                            )
                            .wrap(),
                        );
                        ui.add_space(10.0);
                        cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            cancel |= kit::button(ui, "Annuler").clicked();
                            force |= kit::button(ui, "Forcer le checkout").clicked();
                            ok |= kit::primary_button(ui, "Checkout intelligent").clicked();
                        });
                    }
                    Dialog::Push {
                        remote,
                        branch,
                        target,
                        force,
                        tags,
                        commits,
                        upstream,
                    } => {
                        kit::dialog_title(ui, "Pousser les commits");
                        ui.horizontal(|ui| {
                            let (icon, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                            kit::paint_icon(ui, "branch", icon);
                            ui.label(branch.as_str());
                            paint_arrow_right(ui);
                            let r = kit::icon_text_button(ui, None, remote, true);
                            Popup::menu(&r).show(|ui| {
                                ui.set_min_width(140.0);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for r in &self.snap.remotes {
                                    if menu_item(ui, None, &r.name, "").clicked() {
                                        *remote = r.name.clone();
                                    }
                                }
                            });
                            ui.label(":");
                            ui.add(
                                TextEdit::singleline(target)
                                    .desired_width(180.0)
                                    .margin(Margin::symmetric(8, 5)),
                            );
                            if !*upstream {
                                ui.label(RichText::new("Nouvelle").color(t.success));
                            }
                        });
                        Frame::new()
                            .stroke(stroke(1.0, t.border))
                            .corner_radius(6)
                            .inner_margin(4)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_min_height(120.0);
                                if commits.is_empty() {
                                    secondary(ui, "Aucun commit à pousser");
                                }
                                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for commit in commits.iter() {
                                        let (rect, _) = ui.allocate_exact_size(
                                            vec2(ui.available_width(), kit::ROW),
                                            Sense::hover(),
                                        );
                                        kit::paint_icon(
                                            ui,
                                            "vcsCommit",
                                            Rect::from_min_size(pos2(rect.left() + 4.0, rect.center().y - 8.0), Vec2::splat(16.0)),
                                        );
                                        let (hash, subject) = commit.split_once("  ").unwrap_or(("", commit));
                                        let end = kit::paint_text(ui, rect, rect.left() + 26.0, subject, t.text, body());
                                        kit::paint_text(ui, rect, end + 8.0, hash, t.text_secondary, body());
                                    }
                                });
                            });
                        if !*upstream {
                            secondary(ui, "La branche distante sera créée et deviendra la branche amont.");
                        }
                        ui.horizontal(|ui| {
                            kit::checkbox(ui, force, "Forcer (--force-with-lease)");
                            ui.add_space(12.0);
                            kit::checkbox(ui, tags, "Pousser les tags");
                        });
                        (ok, cancel) = kit::dialog_buttons(ui, "Pousser");
                    }
                    Dialog::Update { rebase, autostash } => {
                        kit::dialog_title(ui, "Mettre à jour le projet");
                        kit::section(ui, "Type de mise à jour");
                        kit::radio_value(ui, rebase, false, "Fusionner les commits entrants (merge)");
                        kit::radio_value(ui, rebase, true, "Rebaser les commits locaux (rebase)");
                        ui.add_space(4.0);
                        kit::checkbox(
                            ui,
                            autostash,
                            "Mettre de côté les modifications locales pendant la mise à jour",
                        );
                        (ok, cancel) = kit::dialog_buttons(ui, "Mettre à jour");
                    }
                    Dialog::Stash { message, untracked } => {
                        kit::dialog_title(ui, "Mettre de côté les modifications");
                        labeled(ui, "Message :", |ui| field(ui, message, "Optionnel")).request_focus();
                        kit::checkbox(ui, untracked, "Inclure les fichiers non versionnés");
                        (ok, cancel) = kit::dialog_buttons(ui, "Mettre de côté");
                    }
                    Dialog::Reset { rev, subject, mode } => {
                        kit::dialog_title(ui, "Réinitialiser la branche courante");
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(short(rev)).color(t.text_secondary));
                            ui.label(subject.as_str());
                        });
                        ui.add_space(4.0);
                        for (value, label, help) in [
                            (ResetMode::Soft, "Soft", "Les modifications des commits annulés restent indexées."),
                            (ResetMode::Mixed, "Mixed", "Les modifications restent dans la copie de travail, non indexées."),
                            (ResetMode::Hard, "Hard", "Toutes les modifications sont perdues."),
                            (ResetMode::Keep, "Keep", "Garde les modifications locales ; échoue si elles sont touchées."),
                        ] {
                            kit::radio_value(ui, mode, value, label);
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                secondary(ui, help);
                            });
                        }
                        (ok, cancel) = kit::dialog_buttons(ui, "Réinitialiser");
                    }
                    Dialog::Clone { url, parent, name } => {
                        kit::dialog_title(ui, "Cloner un dépôt");
                        labeled(ui, "URL :", |ui| {
                            field(ui, url, "https://github.com/utilisateur/projet.git")
                        })
                        .request_focus();
                        labeled(ui, "Dossier parent :", |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if kit::icon_button(ui, "folder", "Choisir le dossier").clicked()
                                    && let Some(dir) = rfd::FileDialog::new()
                                        .set_directory(parent.as_str())
                                        .pick_folder()
                                {
                                    *parent = dir.display().to_string();
                                }
                                field(ui, parent, "Dossier parent");
                            });
                        });
                        labeled(ui, "Nom :", |ui| field(ui, name, "Déduit de l'URL"));
                        secondary(ui, "Le projet cloné s'ouvre à la fin du clonage.");
                        (ok, cancel) = kit::dialog_buttons(ui, "Cloner");
                    }
                    Dialog::Remotes { name, url } => {
                        kit::dialog_title(ui, "Remotes Git");
                        Frame::new()
                            .stroke(stroke(1.0, t.border))
                            .corner_radius(6)
                            .inner_margin(4)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if self.snap.remotes.is_empty() {
                                    secondary(ui, "Aucun remote");
                                }
                                for remote in &self.snap.remotes {
                                    let (rect, response) = tree_row(ui, false);
                                    let end = kit::paint_text(ui, rect, rect.left() + 8.0, &remote.name, t.text, body());
                                    kit::paint_text(ui, rect, end.max(rect.left() + 90.0), &remote.url, t.text_secondary, body());
                                    if response.contains_pointer() {
                                        let button = Rect::from_min_size(
                                            pos2(rect.right() - 28.0, rect.center().y - 11.0),
                                            Vec2::splat(22.0),
                                        );
                                        let r = ui.interact(button, Id::new(("git_remote_remove", &remote.name)), Sense::click());
                                        kit::paint_hover(ui, button, &r, false);
                                        kit::paint_icon(ui, "remove", Rect::from_center_size(button.center(), Vec2::splat(16.0)));
                                        if r.on_hover_text("Supprimer").clicked() {
                                            remove_remote = Some(remote.name.clone());
                                        }
                                    }
                                }
                            });
                        ui.add_space(4.0);
                        kit::section(ui, "Ajouter un remote");
                        labeled(ui, "Nom :", |ui| field(ui, name, "origin"));
                        labeled(ui, "URL :", |ui| field(ui, url, "https://…"));
                        ui.add_space(10.0);
                        cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            cancel |= kit::button(ui, "Fermer").clicked();
                            ok |= kit::primary_button(ui, "Ajouter").clicked();
                        });
                    }
                }
            });
        cancel |= modal.should_close();
        if let Some(remote) = remove_remote {
            self.git("Suppression du remote", &["remote", "remove", &remote]);
        }
        if cancel && !ok && !force {
            return;
        }
        if !ok && !force {
            self.dialog = Some(dialog);
            return;
        }
        match dialog {
            Dialog::Confirm { pending, .. } => {
                self.spawn_task(pending.label, pending.after, pending.task)
            }
            Dialog::NewBranch {
                from,
                name,
                checkout,
            } => self.create_branch(name.trim().to_string(), from, checkout),
            Dialog::RenameBranch { old, name } => {
                let name = name.trim().to_string();
                match repo::ensure_name(&name, "Nom de branche") {
                    Ok(()) => self.git("Renommage", &["branch", "-m", &old, &name]),
                    Err(e) => self.toast(e.to_string(), true),
                }
            }
            Dialog::DeleteBranch { name, force } => self.git(
                "Suppression de la branche",
                &["branch", if force { "-D" } else { "-d" }, &name],
            ),
            Dialog::NewTag { rev, name, message } => {
                let name = name.trim().to_string();
                if let Err(e) = repo::ensure_name(&name, "Nom de tag") {
                    self.toast(e.to_string(), true);
                } else if message.trim().is_empty() {
                    self.git("Nouveau tag", &["tag", &name, &rev]);
                } else {
                    self.git(
                        "Nouveau tag",
                        &["tag", "-a", &name, "-m", message.trim(), &rev],
                    );
                }
            }
            Dialog::Revision { rev } => {
                let rev = rev.trim().to_string();
                if !rev.is_empty() {
                    self.checkout(Target::Revision(rev), host);
                }
            }
            Dialog::SmartCheckout { target } => self.run_checkout(target, ok, force && !ok),
            Dialog::Push {
                remote,
                branch,
                target,
                force,
                tags,
                upstream,
                ..
            } => self.push(
                remote,
                branch,
                target.trim().to_string(),
                force,
                tags,
                upstream,
            ),
            Dialog::Update { rebase, autostash } => self.update(rebase, autostash, host),
            Dialog::Stash { message, untracked } => self.stash(message, untracked, host),
            Dialog::Reset { rev, mode, .. } => {
                if mode == ResetMode::Hard {
                    let pending = self.pending("Reset", After::Nothing, move |git| {
                        git.run(&["reset", "--hard", &rev])
                    });
                    self.confirm(
                        "Reset --hard",
                        "Toutes les modifications locales non commitées seront perdues.".into(),
                        Vec::new(),
                        pending,
                    );
                } else {
                    self.reset(rev, mode);
                }
            }
            Dialog::Clone { url, parent, name } => self.clone_repo(url, parent, name),
            Dialog::Remotes { name, url } => {
                let (name, url) = (name.trim().to_string(), url.trim().to_string());
                if repo::ensure_name(&name, "Nom de remote").is_err() || url.is_empty() {
                    self.toast("Renseignez le nom et l'URL du remote", true);
                } else {
                    self.git("Ajout du remote", &["remote", "add", &name, &url]);
                }
                self.dialog = Some(Dialog::Remotes {
                    name: String::new(),
                    url: String::new(),
                });
            }
        }
    }
}

fn paint_arrow_right(ui: &mut Ui) {
    let t = kit::theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let s = stroke(1.3, t.text_secondary);
    let c = rect.center();
    ui.painter()
        .line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], s);
    ui.painter()
        .line_segment([c + vec2(5.0, 0.0), c + vec2(2.0, -3.0)], s);
    ui.painter()
        .line_segment([c + vec2(5.0, 0.0), c + vec2(2.0, 3.0)], s);
}

fn change_menu(ui: &mut Ui, change: &Change) -> Option<ChangeAction> {
    let path = change.path.clone();
    let mut action = None;
    let mut entry = |ui: &mut Ui, icon: Option<&str>, label: &str, a: ChangeAction| {
        if item(ui, icon, label) {
            action = Some(a);
        }
    };
    if change.kind == Kind::Conflicted {
        entry(
            ui,
            Some("merge"),
            "Résoudre…",
            ChangeAction::Resolve(path.clone()),
        );
        entry(
            ui,
            None,
            "Accepter la version locale (ours)",
            ChangeAction::Accept(path.clone(), false),
        );
        entry(
            ui,
            None,
            "Accepter la version entrante (theirs)",
            ChangeAction::Accept(path.clone(), true),
        );
        entry(
            ui,
            Some("greenCheckmark"),
            "Marquer comme résolu",
            ChangeAction::Resolved(path.clone()),
        );
        menu_separator(ui);
    }
    entry(
        ui,
        Some("diff"),
        "Afficher le diff",
        ChangeAction::Diff(change.clone()),
    );
    if change.kind != Kind::Deleted {
        entry(
            ui,
            Some("edit"),
            "Ouvrir le fichier",
            ChangeAction::Open(path.clone()),
        );
    }
    if change.kind == Kind::Untracked {
        entry(
            ui,
            Some("add"),
            "Ajouter à Git",
            ChangeAction::Add(vec![path.clone()]),
        );
        entry(
            ui,
            Some("hide"),
            "Ajouter au .gitignore",
            ChangeAction::Ignore(path.clone()),
        );
        entry(
            ui,
            Some("delete"),
            "Supprimer le fichier…",
            ChangeAction::Rollback(vec![change.clone()]),
        );
    } else {
        entry(
            ui,
            Some("history"),
            "Historique du fichier",
            ChangeAction::History(path.clone()),
        );
        if change.kind != Kind::Conflicted {
            entry(
                ui,
                Some("rollback"),
                "Annuler les modifications…",
                ChangeAction::Rollback(vec![change.clone()]),
            );
        }
    }
    menu_separator(ui);
    if item(ui, Some("copy"), "Copier le chemin") {
        ui.ctx().copy_text(path);
    }
    action
}

fn commit_menu(
    ui: &mut Ui,
    commit: &Commit,
    head: Option<&str>,
    current: Option<&str>,
) -> Option<LogAction> {
    let hash = commit.hash.clone();
    let merge = commit.parents.len() > 1;
    let mut action = None;
    let mut entry = |ui: &mut Ui, icon: Option<&str>, label: &str, a: LogAction| {
        if item(ui, icon, label) {
            action = Some(a);
        }
    };
    if item(ui, Some("copy"), "Copier le hash") {
        ui.ctx().copy_text(hash.clone());
    }
    menu_separator(ui);
    for r in commit
        .refs
        .iter()
        .filter(|r| r.kind == RefKind::Local && !r.current)
    {
        entry(
            ui,
            Some("branch"),
            &format!("Checkout de « {} »", r.name),
            LogAction::Checkout(Target::Branch(r.name.clone())),
        );
        if let Some(current) = current {
            entry(
                ui,
                Some("merge"),
                &format!("Fusionner « {} » dans « {current} »", r.name),
                LogAction::Merge(r.name.clone()),
            );
        }
    }
    entry(
        ui,
        None,
        "Checkout de la révision",
        LogAction::Checkout(Target::Revision(hash.clone())),
    );
    entry(
        ui,
        Some("add"),
        "Nouvelle branche…",
        LogAction::NewBranch(hash.clone()),
    );
    entry(
        ui,
        Some("tag"),
        "Nouveau tag…",
        LogAction::NewTag(hash.clone()),
    );
    menu_separator(ui);
    entry(
        ui,
        None,
        "Cherry-pick",
        LogAction::CherryPick(hash.clone(), merge),
    );
    entry(
        ui,
        Some("rollback"),
        "Revert du commit",
        LogAction::Revert(hash.clone(), merge),
    );
    entry(
        ui,
        None,
        "Réinitialiser la branche courante ici…",
        LogAction::Reset(hash.clone(), commit.subject.clone()),
    );
    if head == Some(hash.as_str()) && !commit.parents.is_empty() {
        entry(
            ui,
            Some("undo"),
            "Annuler le commit (garder les modifications)",
            LogAction::Undo,
        );
    }
    let tags: Vec<_> = commit
        .refs
        .iter()
        .filter(|r| r.kind == RefKind::Tag)
        .collect();
    if !tags.is_empty() {
        menu_separator(ui);
        for tag in tags {
            entry(
                ui,
                Some("delete"),
                &format!("Supprimer le tag « {} »", tag.name),
                LogAction::DeleteTag(tag.name.clone()),
            );
        }
    }
    action
}

/// One row of the commit graph: lane edges as smooth curves, then the commit's node.
fn paint_graph(painter: &egui::Painter, rect: Rect, row: &graph::GraphRow, head: bool, t: &Theme) {
    let x = |column: usize| rect.left() + 8.0 + column as f32 * LANE_W + LANE_W / 2.0;
    let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());
    let line = |a: Pos2, b: Pos2, color: Color32| {
        let s = stroke(1.5, color);
        if (a.x - b.x).abs() < 0.5 {
            painter.line_segment([a, b], s);
        } else {
            let dy = (b.y - a.y) * 0.55;
            painter.add(CubicBezierShape::from_points_stroke(
                [a, pos2(a.x, a.y + dy), pos2(b.x, b.y - dy), b],
                false,
                Color32::TRANSPARENT,
                s,
            ));
        }
    };
    let node = pos2(x(row.column), mid);
    for edge in &row.edges {
        match *edge {
            Edge::Through {
                top: a,
                bottom: b,
                color,
            } => line(pos2(x(a), top), pos2(x(b), bottom), lane_color(color)),
            Edge::ToNode { top: a, color } => line(pos2(x(a), top), node, lane_color(color)),
            Edge::FromNode { bottom: b, color } => {
                line(node, pos2(x(b), bottom), lane_color(color))
            }
        }
    }
    let color = lane_color(row.color);
    if head {
        painter.circle(node, 4.5, color, stroke(1.5, t.text));
    } else {
        painter.circle_filled(node, 3.5, color);
    }
}

struct DiffStyle {
    t: Theme,
    font: FontId,
    char_w: f32,
}

/// Range of characters `[start, end)` in a line.
type Span = (usize, usize);

/// Character ranges that differ between two versions of a modified line.
fn word_spans(old: &str, new: &str) -> (Vec<Span>, Vec<Span>) {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_words(old, new);
    let (mut o, mut n) = (0, 0);
    let (mut left, mut right): (Vec<Span>, Vec<Span>) = (Vec::new(), Vec::new());
    let mut equal = 0;
    let push = |spans: &mut Vec<(usize, usize)>, from: usize, to: usize| match spans.last_mut() {
        Some(last) if last.1 == from => last.1 = to,
        _ => spans.push((from, to)),
    };
    for change in diff.iter_all_changes() {
        let len = change.value().chars().count();
        match change.tag() {
            ChangeTag::Equal => {
                if !change.value().trim().is_empty() {
                    equal += len;
                }
                o += len;
                n += len;
            }
            ChangeTag::Delete => {
                push(&mut left, o, o + len);
                o += len;
            }
            ChangeTag::Insert => {
                push(&mut right, n, n + len);
                n += len;
            }
        }
    }
    // A line rewritten entirely is not highlighted word by word, as in IntelliJ.
    if equal == 0 {
        return (Vec::new(), Vec::new());
    }
    (left, right)
}

fn paint_line(
    painter: &egui::Painter,
    cell: Rect,
    line: Option<&(usize, String)>,
    bg: Color32,
    spans: &[(usize, usize)],
    s: &DiffStyle,
) {
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(cell, 0.0, bg);
    }
    let Some((number, text)) = line else { return };
    let y = cell.center().y;
    painter.text(
        pos2(cell.left() + 40.0, y),
        Align2::RIGHT_CENTER,
        number.to_string(),
        s.font.clone(),
        s.t.line_number,
    );
    let x = cell.left() + 52.0;
    let clip = painter.with_clip_rect(cell.intersect(painter.clip_rect()));
    let highlight = s.t.modified.gamma_multiply(0.28);
    for &(a, b) in spans {
        let r = Rect::from_min_max(
            pos2(x + a as f32 * s.char_w, cell.top() + 1.0),
            pos2(x + b as f32 * s.char_w, cell.bottom() - 1.0),
        );
        clip.rect_filled(r, 2.0, highlight);
    }
    clip.text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        text,
        s.font.clone(),
        s.t.text,
    );
}

fn expand_tabs(line: &Option<(usize, String)>) -> Option<(usize, String)> {
    line.as_ref()
        .map(|(n, text)| (*n, text.replace('\t', "    ")))
}

fn paint_side_by_side(painter: &egui::Painter, rect: Rect, row: &Row, s: &DiffStyle) {
    let t = &s.t;
    const GUTTER: f32 = 18.0;
    let half = (rect.width() - GUTTER) / 2.0;
    let left = Rect::from_min_size(rect.min, vec2(half, rect.height()));
    let right = Rect::from_min_size(
        pos2(rect.left() + half + GUTTER, rect.top()),
        vec2(half, rect.height()),
    );
    let (old, new) = (expand_tabs(&row.old), expand_tabs(&row.new));
    let side = |present: bool, color: Color32| if present { color } else { t.diff_empty };
    let (left_bg, right_bg, gutter) = match row.kind {
        RowKind::Deleted => (t.diff_deleted, t.diff_empty, t.diff_deleted),
        RowKind::Inserted => (t.diff_empty, t.diff_inserted, t.diff_inserted),
        RowKind::Changed => (
            side(old.is_some(), t.diff_modified),
            side(new.is_some(), t.diff_modified),
            t.diff_modified,
        ),
        _ => (
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
        ),
    };
    let (left_spans, right_spans) = match (&old, &new) {
        (Some((_, a)), Some((_, b))) if row.kind == RowKind::Changed => word_spans(a, b),
        _ => (Vec::new(), Vec::new()),
    };
    paint_line(painter, left, old.as_ref(), left_bg, &left_spans, s);
    paint_line(painter, right, new.as_ref(), right_bg, &right_spans, s);
    let middle = Rect::from_min_max(
        pos2(left.right(), rect.top()),
        pos2(right.left(), rect.bottom()),
    );
    if gutter != Color32::TRANSPARENT {
        painter.rect_filled(middle, 0.0, gutter.gamma_multiply(0.7));
    }
    let border = stroke(1.0, t.border.gamma_multiply(0.6));
    painter.vline(middle.left(), rect.y_range(), border);
    painter.vline(middle.right(), rect.y_range(), border);
}

fn paint_unified(painter: &egui::Painter, rect: Rect, row: &Row, s: &DiffStyle) {
    let t = &s.t;
    let bg = match row.kind {
        RowKind::Deleted => t.diff_deleted,
        RowKind::Inserted => t.diff_inserted,
        _ => Color32::TRANSPARENT,
    };
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(rect, 0.0, bg);
    }
    let y = rect.center().y;
    let number =
        |n: &Option<(usize, String)>| n.as_ref().map_or(String::new(), |(n, _)| n.to_string());
    for (x, n) in [(40.0, &row.old), (84.0, &row.new)] {
        painter.text(
            pos2(rect.left() + x, y),
            Align2::RIGHT_CENTER,
            number(n),
            s.font.clone(),
            t.line_number,
        );
    }
    let text = row
        .new
        .as_ref()
        .or(row.old.as_ref())
        .map_or(String::new(), |(_, t)| t.replace('\t', "    "));
    painter.text(
        pos2(rect.left() + 98.0, y),
        Align2::LEFT_CENTER,
        text,
        s.font.clone(),
        t.text,
    );
}

fn conflict_block(ui: &mut Ui, t: &Theme, number: usize, c: &mut conflict::Conflict) {
    Frame::new()
        .stroke(stroke(1.0, t.border))
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Conflit {number}")).font(kit::semibold(13.0)));
                let (text, color) = match c.choice {
                    None => ("non résolu", t.conflict),
                    Some(Choice::Ours) => ("version locale", t.success),
                    Some(Choice::Theirs) => ("version entrante", t.success),
                    Some(Choice::OursThenTheirs) => ("locale puis entrante", t.success),
                    Some(Choice::TheirsThenOurs) => ("entrante puis locale", t.success),
                };
                ui.label(RichText::new(text).color(color));
            });
            ui.columns(2, |columns| {
                let sides = [
                    ("Local", &c.ours_label, &c.ours, Some(Choice::Ours)),
                    ("Entrant", &c.theirs_label, &c.theirs, Some(Choice::Theirs)),
                ];
                for (ui, (title, label, text, side)) in columns.iter_mut().zip(sides) {
                    ui.label(RichText::new(format!("{title} : {label}")).color(t.text_secondary));
                    let chosen = c.choice == side;
                    let fill = match c.choice {
                        None => t.conflict.gamma_multiply(0.12),
                        _ if chosen => t.diff_inserted,
                        _ => t.diff_empty,
                    };
                    Frame::new()
                        .fill(fill)
                        .inner_margin(8)
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let shown = if text.is_empty() {
                                "(vide)"
                            } else {
                                text.trim_end_matches('\n')
                            };
                            ui.label(RichText::new(shown).monospace().color(t.text));
                        });
                }
            });
            if let Some(base) = &c.base {
                egui::CollapsingHeader::new("Ancêtre commun")
                    .id_salt(("git_base", number))
                    .show(ui, |ui| {
                        ui.label(RichText::new(base.trim_end_matches('\n')).monospace());
                    });
            }
            ui.horizontal_wrapped(|ui| {
                kit::radio_value(ui, &mut c.choice, Some(Choice::Ours), "Garder local");
                kit::radio_value(ui, &mut c.choice, Some(Choice::Theirs), "Garder entrant");
                kit::radio_value(
                    ui,
                    &mut c.choice,
                    Some(Choice::OursThenTheirs),
                    "Local puis entrant",
                );
                kit::radio_value(
                    ui,
                    &mut c.choice,
                    Some(Choice::TheirsThenOurs),
                    "Entrant puis local",
                );
                kit::radio_value(ui, &mut c.choice, None, "Non résolu");
            });
        });
    ui.add_space(8.0);
}
