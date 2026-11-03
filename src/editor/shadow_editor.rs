//! The editor of a `box-shadow` or a `text-shadow`: its shadows one by one (added, taken
//! away), a box (or a text) that shows them, which a drag moves the chosen shadow over, the
//! offsets, blur and spread as numbers, `inset`, and the color picker of the shadow's color.

use super::color_picker::{self, Picker, WIDTH};
use crate::{
    languages::css::{
        colors::{Notation, Rgba},
        shadow::{Length, Shadow, Shadows},
    },
    ui::Theme,
};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind, Ui, epaint, vec2};

const PREVIEW_HEIGHT: f32 = 120.0;

pub struct ShadowEditor {
    pub shadows: Shadows,
    selected: usize,
    picker: Picker,
}

fn paint([r, g, b, a]: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl ShadowEditor {
    pub fn new(shadows: Shadows) -> Self {
        let picker = picker_of(&shadows.list[0]);
        Self {
            shadows,
            selected: 0,
            picker,
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.shadows.list.len() - 1);
        self.picker = picker_of(&self.shadows.list[self.selected]);
    }
}

fn picker_of(shadow: &Shadow) -> Picker {
    match &shadow.color {
        Some(color) => Picker::new(color.rgba, color.notation),
        None => Picker::new([0, 0, 0, 255], Notation::Hex),
    }
}

/// An egui shadow for the preview: whole pixels, in the range egui draws.
fn preview_shadow(shadow: &Shadow) -> epaint::Shadow {
    let clamp_i8 = |v: f32| v.round().clamp(-127.0, 127.0) as i8;
    epaint::Shadow {
        offset: [clamp_i8(shadow.x.pixels()), clamp_i8(shadow.y.pixels())],
        blur: shadow
            .blur
            .as_ref()
            .map_or(0.0, Length::pixels)
            .clamp(0.0, 255.0) as u8,
        spread: shadow
            .spread
            .as_ref()
            .map_or(0.0, Length::pixels)
            .clamp(0.0, 255.0) as u8,
        color: paint(shadow.rgba()),
    }
}

/// The box and its shadows, as a browser draws them; the first shadow on top.
fn paint_box(painter: &egui::Painter, area: Rect, shadows: &Shadows) {
    let target = Rect::from_center_size(area.center(), vec2(96.0, 60.0));
    let radius = 8u8;
    for shadow in shadows.list.iter().rev().filter(|s| !s.inset) {
        painter.add(preview_shadow(shadow).as_shape(target, radius));
    }
    painter.rect_filled(target, radius, Color32::WHITE);
    // An inset shadow is the shadow of what is around the box, seen inside it.
    let inside = painter.with_clip_rect(target.intersect(painter.clip_rect()));
    for shadow in shadows.list.iter().rev().filter(|s| s.inset) {
        let mut around = preview_shadow(shadow);
        around.spread = 0;
        let grow = shadow.spread.as_ref().map_or(0.0, Length::pixels).max(0.0);
        let frame = target.shrink(grow);
        let big = 200.0;
        for side in [
            Rect::from_min_max(
                frame.min - vec2(big, big),
                frame.right_top() + vec2(big, 0.0),
            ),
            Rect::from_min_max(
                frame.left_bottom() - vec2(big, 0.0),
                frame.max + vec2(big, big),
            ),
            Rect::from_min_max(frame.min - vec2(big, 0.0), frame.left_bottom()),
            Rect::from_min_max(frame.right_top(), frame.max + vec2(big, 0.0)),
        ] {
            inside.add(around.as_shape(side, 0));
        }
    }
}

/// A text and its shadows (offsets and color: the blur is left to the browser).
fn paint_text(painter: &egui::Painter, area: Rect, shadows: &Shadows) {
    let font = egui::FontId::proportional(44.0);
    for shadow in shadows.list.iter().rev() {
        let offset = vec2(shadow.x.pixels(), shadow.y.pixels());
        let blur = shadow.blur.as_ref().map_or(0.0, Length::pixels);
        let color = paint(shadow.rgba());
        // A blur shows as copies around the shadow, fainter.
        let copies: &[(f32, f32)] = if blur > 0.5 {
            &[(0.0, 0.0), (-0.5, 0.0), (0.5, 0.0), (0.0, -0.5), (0.0, 0.5)]
        } else {
            &[(0.0, 0.0)]
        };
        let faint = color.gamma_multiply(1.0 / copies.len() as f32 * 1.6);
        for (dx, dy) in copies {
            painter.text(
                area.center() + offset + vec2(dx * blur, dy * blur),
                egui::Align2::CENTER_CENTER,
                "Aa",
                font.clone(),
                if copies.len() > 1 { faint } else { color },
            );
        }
    }
    painter.text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        "Aa",
        font,
        Color32::from_gray(40),
    );
}

/// A length as a field: its number, its unit after it.
fn length_field(
    ui: &mut Ui,
    label: &str,
    length: &mut Length,
    range: std::ops::RangeInclusive<f32>,
    t: Theme,
) -> bool {
    ui.label(egui::RichText::new(label).small().color(t.text_secondary));
    let mut value = length.value;
    let unit = if length.unit.is_empty() {
        "px".to_string()
    } else {
        length.unit.clone()
    };
    let changed = ui
        .add(
            egui::DragValue::new(&mut value)
                .range(range)
                .speed(0.5)
                .max_decimals(1)
                .suffix(unit),
        )
        .changed();
    if changed {
        length.set(value);
    }
    changed
}

/// Draws the editor. Whether the shadows changed.
pub fn show(ui: &mut Ui, editor: &mut ShadowEditor, t: Theme) -> bool {
    let before = editor.shadows.clone();
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.set_width(WIDTH);

    // The shadows of the list.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..editor.shadows.list.len() {
            let chosen = i == editor.selected;
            let text = egui::RichText::new((i + 1).to_string())
                .small()
                .color(if chosen { Color32::WHITE } else { t.text });
            let button = egui::Button::new(text)
                .fill(if chosen { t.accent } else { t.field })
                .corner_radius(4.0)
                .min_size(vec2(24.0, 20.0));
            if ui.add(button).clicked() && !chosen {
                editor.select(i);
            }
        }
        if ui
            .add(
                egui::Button::new(egui::RichText::new("+").small())
                    .corner_radius(4.0)
                    .min_size(vec2(24.0, 20.0)),
            )
            .on_hover_text(tr!("editor.shadow_add"))
            .clicked()
        {
            editor.shadows.list.push(Shadow::new());
            editor.select(editor.shadows.list.len() - 1);
        }
        if editor.shadows.list.len() > 1 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::ui::icon_button(ui, "delete", tr!("editor.shadow_remove")).clicked() {
                    let index = editor.selected;
                    editor.shadows.list.remove(index);
                    editor.select(index.saturating_sub(1));
                }
            });
        }
    });

    // The preview: a drag moves the chosen shadow.
    let (area, response) = ui.allocate_exact_size(vec2(WIDTH, PREVIEW_HEIGHT), Sense::drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 4.0, Color32::from_gray(232));
    if editor.shadows.text {
        paint_text(&painter, area, &editor.shadows);
    } else {
        paint_box(&painter, area, &editor.shadows);
    }
    ui.painter().rect_stroke(
        area,
        4.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Outside,
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    if response.dragged() {
        let delta = response.drag_delta();
        let shadow = &mut editor.shadows.list[editor.selected];
        // The pointer moves the shadow by as many pixels, in the unit of the offset.
        let per_pixel = |l: &Length| {
            if l.pixels() != 0.0 {
                l.value / l.pixels()
            } else {
                1.0
            }
        };
        let (fx, fy) = (per_pixel(&shadow.x), per_pixel(&shadow.y));
        let (x, y) = (shadow.x.value + delta.x * fx, shadow.y.value + delta.y * fy);
        shadow.x.set(x);
        shadow.y.set(y);
    }
    response.on_hover_text(tr!("editor.shadow_drag"));

    // The numbers.
    let text_shadow = editor.shadows.text;
    let shadow = &mut editor.shadows.list[editor.selected];
    egui::Grid::new("shadow_fields")
        .num_columns(4)
        .spacing(vec2(6.0, 6.0))
        .show(ui, |ui| {
            length_field(ui, "X", &mut shadow.x, -200.0..=200.0, t);
            length_field(ui, "Y", &mut shadow.y, -200.0..=200.0, t);
            ui.end_row();
            let blur = shadow.blur.get_or_insert_with(|| Length::px(0.0));
            length_field(ui, tr!("editor.shadow_blur"), blur, 0.0..=300.0, t);
            if !text_shadow {
                let spread = shadow.spread.get_or_insert_with(|| Length::px(0.0));
                length_field(ui, tr!("editor.shadow_spread"), spread, -200.0..=200.0, t);
            }
            ui.end_row();
        });
    // A blur or a spread of 0 that was not written stays unwritten.
    if before
        .list
        .get(editor.selected)
        .is_some_and(|b| b.blur.is_none())
        && shadow.blur.as_ref().is_some_and(|b| b.value == 0.0)
    {
        shadow.blur = None;
    }
    if before
        .list
        .get(editor.selected)
        .is_some_and(|b| b.spread.is_none())
        && shadow.spread.as_ref().is_some_and(|s| s.value == 0.0)
    {
        shadow.spread = None;
    }
    if !text_shadow {
        let label = egui::RichText::new("inset").small().monospace();
        ui.checkbox(&mut shadow.inset, label);
    }

    // The color of the shadow.
    ui.separator();
    if color_picker::show(ui, &mut editor.picker, t) {
        let (color, notation) = (editor.picker.rgba(), editor.picker.notation);
        editor.shadows.list[editor.selected].set_color(color, notation);
    }
    editor.shadows != before
}
