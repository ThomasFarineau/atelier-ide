//! The editor of a `filter` (or `backdrop-filter`) and of a `transform`: what they do, shown on
//! a sample (colors for a filter, a box for a transform), and their functions one under the
//! other, each with a slider per argument, to add, take away and move up.

use super::color_picker::{WIDTH, checkerboard, gradient as mesh};
use crate::{
    languages::css::effects::{EffectKind, Effects, spec},
    ui::Theme,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, ecolor::Hsva, vec2};

const PREVIEW_HEIGHT: f32 = 120.0;

pub struct EffectsEditor {
    pub effects: Effects,
}

impl EffectsEditor {
    pub fn new(effects: Effects) -> Self {
        Self { effects }
    }
}

/// Colors of every hue and lightness, filtered.
fn paint_filter(painter: &egui::Painter, area: Rect, effects: &Effects) {
    checkerboard(painter, area, 4.0);
    painter.add(mesh(area, 36, 12, |x, y| {
        let color = Color32::from(Hsva::new(x, (1.0 - y * 0.6).min(1.0), 1.0 - y * 0.7, 1.0));
        let [r, g, b, a] = effects.filter([color.r(), color.g(), color.b(), color.a()]);
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }));
    let blur = effects.blur();
    if blur > 0.0 {
        painter.text(
            area.right_bottom() - vec2(6.0, 4.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("blur {}px", (blur * 10.0).round() / 10.0),
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    }
}

/// The box before (dashed) and after the transform.
fn paint_transform(painter: &egui::Painter, area: Rect, effects: &Effects, t: Theme) {
    painter.rect_filled(area, 4.0, t.field);
    let center = area.center();
    let (w, h) = (60.0, 40.0);
    let corners = [
        (-w / 2.0, -h / 2.0),
        (w / 2.0, -h / 2.0),
        (w / 2.0, h / 2.0),
        (-w / 2.0, h / 2.0),
    ];
    let before: Vec<Pos2> = corners.iter().map(|(x, y)| center + vec2(*x, *y)).collect();
    // The transform, at half size, so that it stays in view.
    let after: Vec<Pos2> = corners
        .iter()
        .map(|(x, y)| {
            let (tx, ty) = effects.transform(*x, *y);
            center + vec2(tx, ty) * 0.5 + vec2(*x, *y) * 0.5
        })
        .collect();
    for i in 0..4 {
        let (a, b) = (before[i], before[(i + 1) % 4]);
        painter.add(egui::Shape::dashed_line(
            &[a, b],
            Stroke::new(1.0_f32, t.text_secondary),
            4.0,
            3.0,
        ));
    }
    painter.add(egui::Shape::convex_polygon(
        after.clone(),
        t.accent.gamma_multiply(0.35),
        Stroke::new(1.5_f32, t.accent),
    ));
    // Its top left corner, to see it turn.
    painter.circle_filled(after[0], 3.0, t.accent);
}

/// Draws the editor. Whether the effects changed.
pub fn show(ui: &mut Ui, editor: &mut EffectsEditor, t: Theme) -> bool {
    let before = editor.effects.clone();
    let kind = editor.effects.kind;
    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
    ui.set_width(WIDTH);

    let (area, _) = ui.allocate_exact_size(vec2(WIDTH, PREVIEW_HEIGHT), Sense::hover());
    let painter = ui.painter_at(area);
    match kind {
        EffectKind::Filter => paint_filter(&painter, area, &editor.effects),
        EffectKind::Transform => paint_transform(&painter, area, &editor.effects, t),
    }
    painter.rect_stroke(
        area,
        4.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );

    // The functions.
    let mut remove = None;
    let mut raise = None;
    for (index, function) in editor.effects.functions.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&function.name)
                    .monospace()
                    .small()
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if crate::ui::icon_button(ui, "close", tr!("editor.effect_remove")).clicked() {
                    remove = Some(index);
                }
                if index > 0 && crate::ui::icon_button(ui, "up", tr!("editor.effect_up")).clicked()
                {
                    raise = Some(index);
                }
            });
        });
        if let Some(raw) = &function.raw {
            ui.label(
                egui::RichText::new(raw)
                    .monospace()
                    .small()
                    .color(t.text_secondary),
            );
            continue;
        }
        let Some(spec) = spec(kind, &function.name) else {
            continue;
        };
        for (i, (label, low, high)) in spec.args.iter().enumerate() {
            let Some(amount) = function.args.get_mut(i) else {
                // `scale(1.2)` has one value for both: the second shows as the first.
                continue;
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().slider_width = WIDTH - 70.0;
                if !label.is_empty() {
                    ui.label(egui::RichText::new(*label).small().color(t.text_secondary));
                }
                // A percentage takes the range of its factor, times 100.
                let (low, high) = if amount.unit == "%" && spec.unit.is_empty() {
                    (low * 100.0, high * 100.0)
                } else {
                    (*low, *high)
                };
                let mut value = amount.value;
                let slider = egui::Slider::new(&mut value, low..=high)
                    .clamping(egui::SliderClamping::Never)
                    .max_decimals(2)
                    .suffix(amount.unit.clone());
                if ui.add(slider).changed() {
                    amount.value = (value * 100.0).round() / 100.0;
                }
            });
        }
    }
    if let Some(index) = remove {
        editor.effects.functions.remove(index);
    }
    if let Some(index) = raise {
        editor.effects.functions.swap(index - 1, index);
    }

    // Adding a function.
    egui::ComboBox::from_id_salt("effects_add")
        .selected_text(tr!("editor.effect_add"))
        .width(WIDTH - 8.0)
        .show_ui(ui, |ui| {
            for spec in kind.specs() {
                if ui.selectable_label(false, spec.name).clicked() {
                    editor.effects.add(spec);
                }
            }
        });
    editor.effects != before
}
