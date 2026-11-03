//! The editor of an easing curve (`cubic-bezier()`, `ease`…), as IntelliJ and the browsers have
//! one: the curve between its two ends, its two control points to drag, the keywords of CSS to
//! start from, the four numbers, and a dot that moves as the curve says.

use super::color_picker::WIDTH;
use crate::{
    languages::css::easing::{Easing, PRESETS},
    ui::Theme,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::time::Duration;

const GRAPH_HEIGHT: f32 = 236.0;
/// The progress shown, below 0 and above 1: a curve may go past its ends.
const LOW: f32 = -0.5;
const HIGH: f32 = 1.5;
/// One run of the moving dot, then a pause.
const RUN: f32 = 1.2;
const PAUSE: f32 = 0.5;

pub struct EasingEditor {
    pub easing: Easing,
    /// The control point being dragged: 1 or 2.
    dragging: Option<u8>,
}

impl EasingEditor {
    pub fn new(easing: Easing) -> Self {
        Self {
            easing,
            dragging: None,
        }
    }

    /// What goes in the text: the keyword when the curve is one, else `cubic-bezier()`.
    pub fn write(&self) -> String {
        self.easing
            .keyword()
            .map_or_else(|| self.easing.write(), str::to_string)
    }
}

/// Draws the editor. Whether the curve changed.
pub fn show(ui: &mut Ui, editor: &mut EasingEditor, t: Theme) -> bool {
    let before = editor.easing;
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.set_width(WIDTH);

    // The keywords.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for (name, easing) in PRESETS {
            let chosen = editor.easing.keyword() == Some(name);
            let text = egui::RichText::new(name).small().color(if chosen {
                Color32::WHITE
            } else {
                t.text
            });
            let button = egui::Button::new(text)
                .fill(if chosen { t.accent } else { t.field })
                .corner_radius(4.0)
                .min_size(vec2(0.0, 20.0));
            if ui.add(button).clicked() {
                editor.easing = easing;
            }
        }
    });

    // The curve: time across, progress up.
    let (area, response) =
        ui.allocate_exact_size(vec2(WIDTH, GRAPH_HEIGHT), Sense::click_and_drag());
    let margin = 14.0;
    let plot = Rect::from_min_max(area.min + vec2(margin, 0.0), area.max - vec2(margin, 0.0));
    let to_screen = |x: f32, y: f32| -> Pos2 {
        pos2(
            plot.left() + x * plot.width(),
            plot.bottom() - (y - LOW) / (HIGH - LOW) * plot.height(),
        )
    };
    let from_screen = |p: Pos2| -> (f32, f32) {
        (
            ((p.x - plot.left()) / plot.width()).clamp(0.0, 1.0),
            (LOW + (plot.bottom() - p.y) / plot.height() * (HIGH - LOW)).clamp(LOW, HIGH),
        )
    };
    let painter = ui.painter_at(area.expand(2.0));
    painter.rect_filled(area, 4.0, t.field);
    // The square from (0, 0) to (1, 1).
    let unit = Rect::from_two_pos(to_screen(0.0, 0.0), to_screen(1.0, 1.0));
    painter.rect_filled(unit, 0.0, t.island);
    painter.rect_stroke(
        unit,
        0.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );
    for i in 1..4 {
        let f = i as f32 / 4.0;
        let grid = Stroke::new(1.0_f32, t.separator);
        painter.line_segment([to_screen(f, 0.0), to_screen(f, 1.0)], grid);
        painter.line_segment([to_screen(0.0, f), to_screen(1.0, f)], grid);
    }
    let e = editor.easing;
    let (start, end) = (to_screen(0.0, 0.0), to_screen(1.0, 1.0));
    let (p1, p2) = (to_screen(e.x1, e.y1), to_screen(e.x2, e.y2));
    // The arms of the control points.
    let arm = Stroke::new(1.5_f32, t.text_secondary);
    painter.line_segment([start, p1], arm);
    painter.line_segment([end, p2], arm);
    // The curve.
    let points: Vec<Pos2> = (0..=64)
        .map(|i| {
            let (x, y) = e.point(i as f32 / 64.0);
            to_screen(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.5_f32, t.accent)));
    for end_point in [start, end] {
        painter.circle_filled(end_point, 3.0, t.text_secondary);
    }
    for (point, hot) in [
        (p1, editor.dragging == Some(1)),
        (p2, editor.dragging == Some(2)),
    ] {
        painter.circle_filled(point, if hot { 7.0 } else { 6.0 }, Color32::WHITE);
        painter.circle_stroke(point, 6.0, Stroke::new(2.0_f32, t.accent));
    }
    // Dragging a point; a press takes the nearest one.
    if let Some(p) = response.interact_pointer_pos() {
        if response.drag_started()
            || response.is_pointer_button_down_on() && editor.dragging.is_none()
        {
            let near = |q: Pos2| q.distance(p);
            editor.dragging = Some(if near(p1) <= near(p2) { 1 } else { 2 });
        }
        if response.dragged() || response.is_pointer_button_down_on() {
            let (x, y) = from_screen(p);
            let (x, y) = ((x * 100.0).round() / 100.0, (y * 100.0).round() / 100.0);
            match editor.dragging {
                Some(1) => (editor.easing.x1, editor.easing.y1) = (x, y),
                Some(2) => (editor.easing.x2, editor.easing.y2) = (x, y),
                _ => {}
            }
        }
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
        editor.dragging = None;
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // The four numbers.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let easing = &mut editor.easing;
        for (value, low, high) in [
            (&mut easing.x1, 0.0, 1.0),
            (&mut easing.y1, -2.0, 3.0),
            (&mut easing.x2, 0.0, 1.0),
            (&mut easing.y2, -2.0, 3.0),
        ] {
            ui.add(
                egui::DragValue::new(value)
                    .range(low..=high)
                    .speed(0.01)
                    .max_decimals(2)
                    .min_decimals(2),
            );
        }
    });

    // A dot that moves as the curve says, next to one that moves evenly.
    let (track, _) = ui.allocate_exact_size(vec2(WIDTH, 28.0), Sense::hover());
    let painter = ui.painter_at(track.expand(2.0));
    let time = ui.input(|i| i.time) as f32 % (RUN + PAUSE);
    let linear = (time / RUN).min(1.0);
    let eased = editor.easing.progress(linear);
    let lane = |row: f32| track.top() + row * track.height();
    let left = track.left() + 8.0;
    let width = track.width() - 16.0;
    for (row, progress, color) in [(0.3, linear, t.text_secondary), (0.75, eased, t.accent)] {
        let y = lane(row);
        painter.line_segment(
            [pos2(left, y), pos2(left + width, y)],
            Stroke::new(1.0_f32, t.separator),
        );
        let x = left + progress.clamp(LOW, HIGH) * width;
        painter.circle_filled(pos2(x, y), 4.5, color);
    }
    ui.ctx().request_repaint_after(Duration::from_millis(16));
    editor.easing != before
}
