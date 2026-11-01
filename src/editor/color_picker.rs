//! The color picker of a color of a style sheet, as IntelliJ has one: a square of saturation
//! and brightness, a bar of hue and one of opacity, the color before and after, its
//! hexadecimal value to type and its channels, and the notation it is written in.

use crate::{
    languages::css::colors::{Notation, Rgba},
    ui::Theme,
};
use eframe::egui::{
    self, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    ecolor::Hsva, pos2, vec2,
};

pub(super) const WIDTH: f32 = 236.0;
const SQUARE_HEIGHT: f32 = 150.0;
const BAR_HEIGHT: f32 = 14.0;
/// Cells of the meshes: enough for smooth gradients.
const STEPS: usize = 24;

/// What the picker keeps between frames.
pub struct Picker {
    /// The color as hue, saturation, value and alpha, for the controls: the hue stays when the
    /// color is gray.
    hsva: Hsva,
    /// The color, exactly: what was read or typed stays as it was, to the unit.
    rgba: Rgba,
    /// The color before the picker changed it.
    original: Rgba,
    /// What the hexadecimal field holds while it is typed in.
    hex: String,
    pub notation: Notation,
}

impl Picker {
    pub fn new(rgba: Rgba, notation: Notation) -> Self {
        Self {
            hsva: Hsva::from_srgba_unmultiplied(rgba),
            rgba,
            original: rgba,
            hex: hex(rgba),
            notation: match notation {
                Notation::Named => Notation::Hex,
                other => other,
            },
        }
    }

    /// The color, exactly.
    pub fn rgba(&self) -> Rgba {
        self.rgba
    }

    /// The color to paint.
    fn color(&self) -> Color32 {
        paint(self.rgba)
    }

    /// A color read or typed.
    fn set(&mut self, rgba: Rgba) {
        // A gray keeps the hue it had.
        let hue = self.hsva.h;
        self.rgba = rgba;
        self.hsva = Hsva::from_srgba_unmultiplied(rgba);
        if self.hsva.s == 0.0 || self.hsva.v == 0.0 {
            self.hsva.h = hue;
        }
    }

    /// The controls moved the color.
    fn moved(&mut self) {
        self.rgba = self.hsva.to_srgba_unmultiplied();
    }
}

fn paint([r, g, b, a]: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn hex(rgba: Rgba) -> String {
    let [r, g, b, a] = rgba;
    if a == 255 {
        format!("{r:02X}{g:02X}{b:02X}")
    } else {
        format!("{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// The square under a color that lets light through.
pub(super) fn checkerboard(painter: &egui::Painter, rect: Rect, radius: f32) {
    painter.rect_filled(rect, radius, Color32::WHITE);
    let cell = 5.0;
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut y = rect.top();
    let mut row = 0;
    while y < rect.bottom() {
        let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { cell };
        while x < rect.right() {
            clip.rect_filled(
                Rect::from_min_size(pos2(x, y), vec2(cell, cell)).intersect(rect),
                0.0,
                Color32::from_gray(204),
            );
            x += cell * 2.0;
        }
        y += cell;
        row += 1;
    }
}

/// A grid of colored cells: `color(x, y)` with x and y from 0 to 1.
pub(super) fn gradient(
    rect: Rect,
    columns: usize,
    rows: usize,
    color: impl Fn(f32, f32) -> Color32,
) -> Shape {
    let mut mesh = Mesh::default();
    for j in 0..=rows {
        for i in 0..=columns {
            let (x, y) = (i as f32 / columns as f32, j as f32 / rows as f32);
            mesh.colored_vertex(
                pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                ),
                color(x, y),
            );
        }
    }
    let stride = columns as u32 + 1;
    for j in 0..rows as u32 {
        for i in 0..columns as u32 {
            let a = j * stride + i;
            mesh.add_triangle(a, a + 1, a + stride);
            mesh.add_triangle(a + 1, a + stride + 1, a + stride);
        }
    }
    Shape::mesh(mesh)
}

/// The round handle of the square: the color it points at, in a white ring with a shadow.
fn ring(painter: &egui::Painter, at: Pos2, color: Color32) {
    painter.circle_filled(at + vec2(0.0, 1.0), 8.5, Color32::from_black_alpha(70));
    painter.circle_filled(at, 7.0, color);
    painter.circle_stroke(at, 7.0, Stroke::new(2.5_f32, Color32::WHITE));
    painter.circle_stroke(at, 8.5, Stroke::new(1.0_f32, Color32::from_black_alpha(90)));
}

/// The handle of a bar: a white knob, taller than the bar, filled with `color`.
pub(super) fn knob(painter: &egui::Painter, x: f32, bar: Rect, color: Color32) {
    let rect = Rect::from_center_size(pos2(x, bar.center().y), vec2(10.0, bar.height() + 8.0));
    painter.rect_filled(
        rect.translate(vec2(0.0, 1.0)).expand(1.0),
        4.0,
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(rect, 4.0, Color32::WHITE);
    let inner = rect.shrink(2.5);
    if color.a() < 255 {
        checkerboard(painter, inner, 2.0);
    }
    painter.rect_filled(inner, 2.0, color);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, Color32::from_black_alpha(90)),
        StrokeKind::Outside,
    );
}

/// A bar of the picker (hue or alpha), its knob filled with `knob_color`: where it was
/// dragged to, from 0 to 1.
fn bar(
    ui: &mut Ui,
    value: f32,
    knob_color: Color32,
    t: Theme,
    paint: impl FnOnce(&egui::Painter, Rect),
) -> Option<f32> {
    // Room for the knob at both ends and above and below.
    let (outer, response) =
        ui.allocate_exact_size(vec2(WIDTH, BAR_HEIGHT + 8.0), Sense::click_and_drag());
    let rect = Rect::from_center_size(outer.center(), vec2(WIDTH - 10.0, BAR_HEIGHT));
    let painter = ui.painter_at(outer.expand(2.0));
    paint(&painter, rect);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Outside,
    );
    let x = rect.left() + value.clamp(0.0, 1.0) * rect.width();
    knob(&painter, x, rect, knob_color);
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
        .interact_pointer_pos()
        .filter(|_| {
            response.dragged() || response.clicked() || response.is_pointer_button_down_on()
        })
        .map(|p| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0))
}

/// Draws the picker. Whether the color changed.
pub fn show(ui: &mut Ui, picker: &mut Picker, t: Theme) -> bool {
    let before = picker.rgba();
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.set_width(WIDTH);

    // Saturation (across) and brightness (down).
    let (square, response) =
        ui.allocate_exact_size(vec2(WIDTH, SQUARE_HEIGHT), Sense::click_and_drag());
    let hue = picker.hsva.h;
    let painter = ui.painter_at(square.expand(6.0));
    painter.add(gradient(square, STEPS, STEPS, |x, y| {
        Color32::from(Hsva::new(hue, x, 1.0 - y, 1.0))
    }));
    painter.rect_stroke(
        square,
        0.0,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Outside,
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if let Some(p) = response.interact_pointer_pos()
        && (response.dragged() || response.is_pointer_button_down_on())
    {
        picker.hsva.s = ((p.x - square.left()) / square.width()).clamp(0.0, 1.0);
        picker.hsva.v = 1.0 - ((p.y - square.top()) / square.height()).clamp(0.0, 1.0);
        picker.moved();
    }
    ring(
        &painter,
        pos2(
            square.left() + picker.hsva.s * square.width(),
            square.top() + (1.0 - picker.hsva.v) * square.height(),
        ),
        Color32::from(Hsva {
            a: 1.0,
            ..picker.hsva
        }),
    );

    // Hue.
    let pure = Color32::from(Hsva::new(picker.hsva.h, 1.0, 1.0, 1.0));
    if let Some(h) = bar(ui, picker.hsva.h, pure, t, |painter, rect| {
        painter.add(gradient(rect, 36, 1, |x, _| {
            Color32::from(Hsva::new(x, 1.0, 1.0, 1.0))
        }));
    }) {
        picker.hsva.h = h.min(0.9999);
        picker.moved();
    }

    // Opacity, over a checkerboard.
    let opaque = Color32::from(Hsva {
        a: 1.0,
        ..picker.hsva
    });
    if let Some(a) = bar(ui, picker.hsva.a, picker.color(), t, |painter, rect| {
        checkerboard(painter, rect, 0.0);
        painter.add(gradient(rect, 1, 1, |x, _| opaque.gamma_multiply(x)));
    }) {
        picker.hsva.a = a;
        picker.moved();
    }

    // The color before and after, and its hexadecimal value.
    ui.horizontal(|ui| {
        let (preview, _) = ui.allocate_exact_size(vec2(56.0, 28.0), Sense::hover());
        checkerboard(ui.painter(), preview, 4.0);
        let half = Rect::from_min_max(preview.min, pos2(preview.center().x, preview.bottom()));
        let back = ui
            .interact(half, ui.id().with("original"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr!("editor.color_original"));
        if back.clicked() {
            picker.set(picker.original);
            picker.hex = hex(picker.original);
        }
        ui.painter().rect_filled(
            half,
            CornerRadius {
                nw: 4,
                sw: 4,
                ne: 0,
                se: 0,
            },
            paint(picker.original),
        );
        let other = Rect::from_min_max(pos2(preview.center().x, preview.top()), preview.max);
        ui.painter().rect_filled(
            other,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: 4,
                se: 4,
            },
            picker.color(),
        );
        ui.painter().rect_stroke(
            preview,
            4.0,
            Stroke::new(1.0_f32, t.border),
            StrokeKind::Outside,
        );
        ui.label(egui::RichText::new("#").color(t.text_secondary).monospace());
        let field = ui.add(
            egui::TextEdit::singleline(&mut picker.hex)
                .font(egui::TextStyle::Monospace)
                .desired_width(WIDTH - 56.0 - 40.0)
                .char_limit(8),
        );
        if field.changed() {
            let typed = format!("#{}", picker.hex.trim().trim_start_matches('#'));
            if let Some(color) = crate::languages::css::colors::parse_hex(&typed) {
                picker.set(color);
            }
        }
        if !field.has_focus() {
            picker.hex = hex(picker.rgba());
        }
    });

    // The channels.
    let [mut r, mut g, mut b, a] = picker.rgba();
    let mut alpha = (a as f32 / 255.0 * 100.0).round() as u8;
    let mut typed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (label, value, max) in [
            ("R", &mut r, 255u8),
            ("G", &mut g, 255),
            ("B", &mut b, 255),
            ("A%", &mut alpha, 100),
        ] {
            ui.label(egui::RichText::new(label).small().color(t.text_secondary));
            typed |= ui
                .add(egui::DragValue::new(value).range(0..=max).speed(1.0))
                .changed();
        }
    });
    if typed {
        let a = (alpha as f32 / 100.0 * 255.0).round() as u8;
        picker.set([r, g, b, a]);
    }

    // How it is written.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (notation, label) in [
            (Notation::Hex, "HEX"),
            (Notation::Rgb, "RGB"),
            (Notation::Hsl, "HSL"),
        ] {
            let chosen = picker.notation == notation;
            let text = egui::RichText::new(label).small().color(if chosen {
                Color32::WHITE
            } else {
                t.text
            });
            let button = egui::Button::new(text)
                .fill(if chosen { t.accent } else { t.field })
                .corner_radius(4.0)
                .min_size(vec2(44.0, 20.0));
            if ui.add(button).clicked() && !chosen {
                picker.notation = notation;
                typed = true;
            }
        }
    });
    typed || picker.rgba() != before
}
