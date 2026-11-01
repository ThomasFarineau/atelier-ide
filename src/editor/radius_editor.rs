//! The editor of a `border-radius`: a box with its corners, whose handles a drag rounds, the
//! four corners as numbers, and a lock that keeps them the same.

use super::color_picker::WIDTH;
use crate::{
    languages::css::{radius::Radius, shadow::Length},
    ui::Theme,
};
use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

const PREVIEW_HEIGHT: f32 = 140.0;
const BOX: egui::Vec2 = egui::Vec2::new(150.0, 100.0);

pub struct RadiusEditor {
    pub radius: Radius,
    /// The four corners change together.
    linked: bool,
    dragging: Option<usize>,
}

impl RadiusEditor {
    pub fn new(radius: Radius) -> Self {
        let linked = radius.corners.iter().all(|c| *c == radius.corners[0]);
        Self {
            radius,
            linked,
            dragging: None,
        }
    }

    fn set(&mut self, corner: usize, value: f32) {
        let value = value.max(0.0);
        if self.linked {
            let mut length = self.radius.corners[corner].clone();
            length.set(value);
            self.radius.corners = std::array::from_fn(|_| length.clone());
        } else {
            self.radius.corners[corner].set(value);
        }
    }
}

/// Draws the editor. Whether the radius changed.
pub fn show(ui: &mut Ui, editor: &mut RadiusEditor, t: Theme) -> bool {
    let before = editor.radius.clone();
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.set_width(WIDTH);

    let (area, response) =
        ui.allocate_exact_size(vec2(WIDTH, PREVIEW_HEIGHT), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 4.0, t.field);
    let target = Rect::from_center_size(area.center(), BOX);
    let px = |corner: usize| {
        editor
            .radius
            .pixels(corner, BOX.x, BOX.y)
            .clamp(0.0, BOX.x.min(BOX.y) / 2.0)
    };
    let corner_radius = CornerRadius {
        nw: px(0).round() as u8,
        ne: px(1).round() as u8,
        se: px(2).round() as u8,
        sw: px(3).round() as u8,
    };
    painter.rect_filled(target, corner_radius, t.accent.gamma_multiply(0.35));
    painter.rect_stroke(
        target,
        corner_radius,
        Stroke::new(1.5_f32, t.accent),
        StrokeKind::Inside,
    );
    // A handle on each corner, as far in as it is round, along the edge.
    let corners = [
        (target.left_top(), vec2(1.0, 0.0)),
        (target.right_top(), vec2(-1.0, 0.0)),
        (target.right_bottom(), vec2(-1.0, 0.0)),
        (target.left_bottom(), vec2(1.0, 0.0)),
    ];
    let handle = |i: usize| corners[i].0 + corners[i].1 * px(i);
    for i in 0..4 {
        let hot = editor.dragging == Some(i);
        painter.circle_filled(handle(i), if hot { 6.5 } else { 5.5 }, Color32::WHITE);
        painter.circle_stroke(handle(i), 5.5, Stroke::new(2.0_f32, t.accent));
    }
    if let Some(p) = response.interact_pointer_pos() {
        if response.drag_started() || (response.clicked() && editor.dragging.is_none()) {
            editor.dragging =
                (0..4).min_by(|&a, &b| handle(a).distance(p).total_cmp(&handle(b).distance(p)));
        }
        if let Some(i) = editor.dragging
            && (response.dragged() || response.clicked())
        {
            // The distance from the corner, along its edge.
            let pixels = ((p.x - corners[i].0.x) * corners[i].1.x).clamp(0.0, BOX.y / 2.0);
            let length = &editor.radius.corners[i];
            let value = if length.unit == "%" {
                pixels / BOX.x.min(BOX.y) * 100.0
            } else if length.pixels() != 0.0 {
                pixels * length.value / length.pixels()
            } else {
                pixels
            };
            editor.set(i, value.round());
        }
    }
    if response.drag_stopped() {
        editor.dragging = None;
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response.on_hover_text(tr!("editor.radius_drag"));

    // The corners as numbers, where they are on the box.
    let field = |ui: &mut Ui, editor: &mut RadiusEditor, corner: usize| {
        let length: &Length = &editor.radius.corners[corner];
        let mut value = length.value;
        let unit = if length.unit.is_empty() {
            "px".to_string()
        } else {
            length.unit.clone()
        };
        if ui
            .add(
                egui::DragValue::new(&mut value)
                    .range(0.0..=999.0)
                    .speed(0.5)
                    .max_decimals(2)
                    .suffix(unit),
            )
            .changed()
        {
            editor.set(corner, value);
        }
    };
    ui.horizontal(|ui| {
        field(ui, editor, 0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            field(ui, editor, 1);
        });
    });
    ui.horizontal(|ui| {
        field(ui, editor, 3);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            field(ui, editor, 2);
        });
    });
    ui.horizontal(|ui| {
        let icon = if editor.linked {
            "locked"
        } else {
            "unversioned"
        };
        if crate::ui::icon_toggle(ui, icon, editor.linked, tr!("editor.radius_link")).clicked() {
            editor.linked = !editor.linked;
            if editor.linked {
                let first = editor.radius.corners[0].clone();
                editor.radius.corners = std::array::from_fn(|_| first.clone());
            }
        }
        ui.label(
            egui::RichText::new(tr!("editor.radius_link"))
                .small()
                .color(t.text_secondary),
        );
    });
    editor.radius != before
}
