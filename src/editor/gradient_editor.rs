//! The editor of a CSS gradient, opened from its swatch as the color picker is from a color's:
//! the gradient as it looks, which a drag turns (linear and conic); its kind, its repetition and
//! its shape; the bar of its stops, where a click adds one, a drag moves one and a drag down
//! takes one away; and the color picker of the chosen stop.

use super::color_picker::{self, Picker, WIDTH, checkerboard, gradient as mesh};
use crate::{
    languages::css::{
        colors::Rgba,
        gradient::{Gradient, Kind, Position, Stop},
    },
    ui::Theme,
};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

const PREVIEW_HEIGHT: f32 = 84.0;
const BAR_HEIGHT: f32 = 16.0;
/// How far below the bar a stop is dragged to take it away.
const REMOVE_DISTANCE: f32 = 28.0;

pub struct GradientEditor {
    pub gradient: Gradient,
    selected: usize,
    picker: Picker,
    /// The stop being dragged on the bar.
    dragging: Option<usize>,
}

fn paint([r, g, b, a]: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl GradientEditor {
    pub fn new(gradient: Gradient) -> Self {
        let first = &gradient.stops[0];
        let picker = Picker::new(first.color, first.notation);
        Self {
            gradient,
            selected: 0,
            picker,
            dragging: None,
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.gradient.stops.len() - 1);
        let stop = &self.gradient.stops[self.selected];
        self.picker = Picker::new(stop.color, stop.notation);
    }

    /// The stops in the order of their places, the chosen one followed.
    fn sort(&mut self) {
        let positions = self.gradient.positions();
        let mut order: Vec<usize> = (0..self.gradient.stops.len()).collect();
        order.sort_by(|&a, &b| positions[a].total_cmp(&positions[b]));
        if order.iter().enumerate().all(|(i, &o)| i == o) {
            return;
        }
        let stops = std::mem::take(&mut self.gradient.stops);
        self.selected = order.iter().position(|&o| o == self.selected).unwrap_or(0);
        self.dragging = self
            .dragging
            .and_then(|d| order.iter().position(|&o| o == d));
        self.gradient.stops = order.into_iter().map(|o| stops[o].clone()).collect();
    }
}

/// A button of a row of choices.
fn choice(ui: &mut Ui, label: &str, chosen: bool, t: Theme) -> bool {
    let text =
        egui::RichText::new(label)
            .small()
            .color(if chosen { Color32::WHITE } else { t.text });
    let button = egui::Button::new(text)
        .fill(if chosen { t.accent } else { t.field })
        .corner_radius(4.0)
        .min_size(vec2(0.0, 20.0));
    ui.add(button).clicked() && !chosen
}

/// The handle of a stop under the bar: a small tab filled with its color.
fn stop_handle(
    painter: &egui::Painter,
    x: f32,
    top: f32,
    color: Color32,
    selected: bool,
    t: Theme,
) {
    let rect = Rect::from_min_size(pos2(x - 6.0, top), vec2(12.0, 14.0));
    let tip = [
        pos2(x, top - 5.0),
        pos2(x - 5.0, top + 1.0),
        pos2(x + 5.0, top + 1.0),
    ];
    let outline = if selected { t.accent } else { Color32::WHITE };
    painter.add(egui::Shape::convex_polygon(
        tip.to_vec(),
        outline,
        Stroke::NONE,
    ));
    painter.rect_filled(rect.expand(1.0), 3.0, Color32::from_black_alpha(90));
    painter.rect_filled(rect, 3.0, outline);
    let inner = rect.shrink(2.5);
    if color.a() < 255 {
        checkerboard(painter, inner, 1.5);
    }
    painter.rect_filled(inner, 1.5, color);
}

/// Draws the editor. Whether the gradient changed.
pub fn show(ui: &mut Ui, editor: &mut GradientEditor, t: Theme) -> bool {
    let before = editor.gradient.clone();
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.set_width(WIDTH);

    // The kind, and whether it repeats.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (kind, label) in [
            (Kind::Linear, tr!("editor.gradient_linear")),
            (Kind::Radial, tr!("editor.gradient_radial")),
            (Kind::Conic, tr!("editor.gradient_conic")),
        ] {
            if choice(ui, label, editor.gradient.kind == kind, t) {
                // The head of one kind means nothing to another.
                editor.gradient.kind = kind;
                editor.gradient.head = None;
            }
        }
        ui.add_space(6.0);
        // Repeated or not: a switch.
        let repeating = editor.gradient.repeating;
        let text = egui::RichText::new(tr!("editor.gradient_repeat"))
            .small()
            .color(if repeating { Color32::WHITE } else { t.text });
        let button = egui::Button::new(text)
            .fill(if repeating { t.accent } else { t.field })
            .corner_radius(4.0)
            .min_size(vec2(0.0, 20.0));
        if ui.add(button).clicked() {
            editor.gradient.repeating = !repeating;
        }
    });

    // The gradient as it looks; for a linear or conic one, a drag turns it.
    let (preview, response) =
        ui.allocate_exact_size(vec2(WIDTH, PREVIEW_HEIGHT), Sense::click_and_drag());
    let painter = ui.painter_at(preview.expand(4.0));
    checkerboard(&painter, preview, 4.0);
    {
        let g = &editor.gradient;
        let (w, h) = (preview.width(), preview.height());
        painter.add(mesh(preview, 40, 16, |x, y| {
            paint(g.color_at(x * w, y * h, w, h))
        }));
    }
    painter.rect_stroke(
        preview,
        4.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Outside,
    );
    let turns = matches!(editor.gradient.kind, Kind::Linear | Kind::Conic);
    if let Some(angle) = editor.gradient.angle().filter(|_| turns) {
        // The direction, from the center.
        let (sin, cos) = angle.to_radians().sin_cos();
        let center = preview.center();
        let reach = preview.height() / 2.0 - 8.0;
        let tip = center + vec2(sin, -cos) * reach;
        painter.line_segment(
            [center, tip],
            Stroke::new(3.0_f32, Color32::from_black_alpha(90)),
        );
        painter.line_segment([center, tip], Stroke::new(1.5_f32, Color32::WHITE));
        painter.circle_filled(tip, 4.0, Color32::WHITE);
        painter.circle_stroke(
            tip,
            4.0,
            Stroke::new(1.0_f32, Color32::from_black_alpha(120)),
        );
    }
    if turns {
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if let Some(p) = response.interact_pointer_pos()
            && (response.dragged() || response.is_pointer_button_down_on())
        {
            let d = p - preview.center();
            if d.length() > 2.0 {
                let mut angle = d.x.atan2(-d.y).to_degrees().rem_euclid(360.0);
                // Shift keeps to the steps of 15°.
                if ui.input(|i| i.modifiers.shift) {
                    angle = (angle / 15.0).round() * 15.0;
                }
                editor.gradient.set_angle(angle.round());
            }
        }
        response.on_hover_text(tr!("editor.gradient_turn"));
    }

    // The angle as a number, or the shape.
    ui.horizontal(|ui| match editor.gradient.kind {
        Kind::Linear | Kind::Conic => {
            let label = if editor.gradient.kind == Kind::Linear {
                tr!("editor.gradient_angle")
            } else {
                tr!("editor.gradient_from")
            };
            ui.label(egui::RichText::new(label).small().color(t.text_secondary));
            match editor.gradient.angle() {
                Some(angle) => {
                    let mut value = angle;
                    let field = egui::DragValue::new(&mut value)
                        .range(0.0..=360.0)
                        .speed(1.0)
                        .suffix("°");
                    if ui.add(field).changed() {
                        editor.gradient.set_angle(value);
                    }
                }
                None => {
                    let head = editor.gradient.head.clone().unwrap_or_default();
                    ui.label(egui::RichText::new(head).small().monospace());
                }
            }
        }
        Kind::Radial => {
            ui.spacing_mut().item_spacing.x = 2.0;
            let circle = editor.gradient.circle();
            if choice(ui, tr!("editor.gradient_ellipse"), !circle, t) {
                editor.gradient.set_circle(false);
            }
            if choice(ui, tr!("editor.gradient_circle"), circle, t) {
                editor.gradient.set_circle(true);
            }
        }
    });

    // The stops: the bar of the gradient, their handles under it.
    let (outer, bar_response) =
        ui.allocate_exact_size(vec2(WIDTH, BAR_HEIGHT + 22.0), Sense::click_and_drag());
    let bar = Rect::from_min_size(outer.min + vec2(6.0, 0.0), vec2(WIDTH - 12.0, BAR_HEIGHT));
    let painter = ui.painter_at(outer.expand(6.0));
    checkerboard(&painter, bar, 3.0);
    {
        let g = &editor.gradient;
        painter.add(mesh(bar, 64, 1, |x, _| paint(g.sample(x))));
    }
    painter.rect_stroke(
        bar,
        3.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Outside,
    );
    let positions = editor.gradient.positions();
    let x_of = |p: f32| bar.left() + p.clamp(0.0, 1.0) * bar.width();
    let handle_top = bar.bottom() + 6.0;
    let pointer = bar_response.interact_pointer_pos();
    if (bar_response.drag_started() || bar_response.clicked())
        && let Some(p) = pointer
    {
        // The stop under the pointer, else a new one there, of the color the gradient has there.
        let hit = positions
            .iter()
            .enumerate()
            .filter(|(_, pos)| (x_of(**pos) - p.x).abs() <= 7.0)
            .min_by(|a, b| {
                (x_of(*a.1) - p.x)
                    .abs()
                    .total_cmp(&(x_of(*b.1) - p.x).abs())
            })
            .map(|(i, _)| i);
        match hit {
            Some(i) => editor.select(i),
            None => {
                let at = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
                let color = editor.gradient.sample(at);
                let notation = editor.gradient.stops[editor.selected].notation;
                let index = positions.partition_point(|&q| q <= at);
                let percent = (at * 1000.0).round() / 10.0;
                editor
                    .gradient
                    .stops
                    .insert(index, Stop::new(color, notation, percent));
                editor.select(index);
            }
        }
        if bar_response.drag_started() {
            editor.dragging = Some(editor.selected);
        }
    }
    let mut removing = false;
    if let (Some(index), Some(p)) = (editor.dragging, pointer)
        && bar_response.dragged()
    {
        removing = editor.gradient.stops.len() > 2 && p.y > handle_top + REMOVE_DISTANCE;
        let at = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
        editor.gradient.stops[index].position =
            Some(Position::Percent((at * 1000.0).round() / 10.0));
        editor.sort();
    }
    if bar_response.drag_stopped() {
        if let (Some(index), Some(p)) = (editor.dragging, ui.input(|i| i.pointer.latest_pos()))
            && editor.gradient.stops.len() > 2
            && p.y > handle_top + REMOVE_DISTANCE
        {
            editor.gradient.stops.remove(index);
            editor.select(index.saturating_sub(1));
        }
        editor.dragging = None;
    }
    if bar_response.hovered() || bar_response.dragged() {
        ui.ctx().set_cursor_icon(if removing {
            egui::CursorIcon::NotAllowed
        } else {
            egui::CursorIcon::PointingHand
        });
    }
    let positions = editor.gradient.positions();
    for (i, stop) in editor.gradient.stops.iter().enumerate() {
        if removing && Some(i) == editor.dragging {
            continue;
        }
        let selected = i == editor.selected;
        stop_handle(
            &painter,
            x_of(positions[i]),
            handle_top,
            paint(stop.color),
            selected,
            t,
        );
    }
    bar_response.on_hover_text(tr!("editor.gradient_stops"));

    // The chosen stop: its place, and taking it away.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(tr!("editor.gradient_position"))
                .small()
                .color(t.text_secondary),
        );
        let stop = &mut editor.gradient.stops[editor.selected];
        match stop.position.clone() {
            Some(Position::Other(text)) => {
                ui.label(egui::RichText::new(text).small().monospace());
            }
            position => {
                let mut value = match position {
                    Some(Position::Percent(p)) => p,
                    _ => positions[editor.selected] * 100.0,
                };
                let field = egui::DragValue::new(&mut value)
                    .range(0.0..=100.0)
                    .speed(0.5)
                    .max_decimals(1)
                    .suffix("%");
                if ui.add(field).changed() {
                    stop.position = Some(Position::Percent(value));
                    editor.sort();
                }
            }
        }
        if editor.gradient.stops.len() > 2 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::ui::icon_button(ui, "delete", tr!("editor.gradient_remove_stop"))
                    .clicked()
                {
                    let index = editor.selected;
                    editor.gradient.stops.remove(index);
                    editor.select(index.saturating_sub(1));
                }
            });
        }
    });

    // The color of the chosen stop.
    ui.separator();
    if color_picker::show(ui, &mut editor.picker, t) {
        let (color, notation) = (editor.picker.rgba(), editor.picker.notation);
        editor.gradient.stops[editor.selected].set_color(color, notation);
    }
    editor.gradient != before
}
