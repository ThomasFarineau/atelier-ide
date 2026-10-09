//! IntelliJ "Islands" look and feel, shared by the IDE and its built-in plugins: theme colors,
//! fonts (Inter, JetBrains Mono), the IntelliJ icon set and the widgets egui draws differently.
//!
//! Plugins read the current theme with [`theme`] and draw with the helpers below, so every part
//! of the interface follows the same metrics: 13 px text, 16 px icons, 24 px rows, 4 to 8 px radii.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    ImageSource, Pos2, Rect, Response, RichText, Sense, Shadow, Stroke, StrokeKind, Ui, Vec2,
    load::Bytes, pos2, vec2,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

pub const THEMES: [&str; 3] = ["Islands Dark", "Dark", "Islands Light"];

/// Height of tree, list and table rows.
pub const ROW: f32 = 24.0;
/// Size of toolbar icon buttons.
pub const BUTTON: f32 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// Title bar, tool window stripes, status bar and the gaps between islands.
    pub frame: Color32,
    /// Tool windows and editor.
    pub island: Color32,
    pub popup: Color32,
    /// Borders of controls and of the active editor tab.
    pub border: Color32,
    pub separator: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    /// Selected row in a focused tree or list.
    pub selection: Color32,
    /// Selected row when the list is not focused.
    pub selection_inactive: Color32,
    /// Button of an open tool window that does not have the focus.
    pub tool_open: Color32,
    /// Primary buttons, focused tool window button, focus rings.
    pub accent: Color32,
    pub accent_hover: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_disabled: Color32,
    pub link: Color32,
    pub line_number: Color32,
    pub line_number_current: Color32,
    pub caret_row: Color32,
    pub text_selection: Color32,
    pub field: Color32,
    // File status colors.
    pub modified: Color32,
    pub added: Color32,
    pub deleted: Color32,
    pub unversioned: Color32,
    pub conflict: Color32,
    // Diff backgrounds.
    pub diff_inserted: Color32,
    pub diff_deleted: Color32,
    pub diff_modified: Color32,
    pub diff_empty: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const ISLANDS_DARK: Theme = Theme {
    dark: true,
    frame: hex(0x26282C),
    island: hex(0x191A1C),
    popup: hex(0x2B2D30),
    border: hex(0x40434A),
    separator: hex(0x2B2D30),
    hover: hex(0x2F3135),
    pressed: hex(0x393B40),
    selection: hex(0x2E436E),
    selection_inactive: hex(0x2F3135),
    tool_open: hex(0x4A4D50),
    accent: hex(0x3871E1),
    accent_hover: hex(0x4682FA),
    text: hex(0xDFE1E5),
    text_secondary: hex(0x868A91),
    text_disabled: hex(0x5A5D63),
    link: hex(0x548AF7),
    line_number: hex(0x4B5059),
    line_number_current: hex(0xA1A3AB),
    caret_row: hex(0x202124),
    text_selection: hex(0x214283),
    field: hex(0x191A1C),
    modified: hex(0x70AEFF),
    added: hex(0x73BD79),
    deleted: hex(0x868A91),
    unversioned: hex(0xE0716C),
    conflict: hex(0xF0605B),
    diff_inserted: hex(0x253A2B),
    diff_deleted: hex(0x3B3D40),
    diff_modified: hex(0x233452),
    diff_empty: hex(0x212225),
    success: hex(0x5FB865),
    warning: hex(0xF2C55C),
    error: hex(0xF0605B),
};

const DARK: Theme = Theme {
    frame: hex(0x2B2D30),
    island: hex(0x1E1F22),
    popup: hex(0x2B2D30),
    border: hex(0x43454A),
    separator: hex(0x393B40),
    hover: hex(0x393B40),
    pressed: hex(0x43454A),
    selection_inactive: hex(0x393B40),
    tool_open: hex(0x43454A),
    accent: hex(0x3574F0),
    caret_row: hex(0x26282E),
    field: hex(0x1E1F22),
    diff_empty: hex(0x26272A),
    ..ISLANDS_DARK
};

const ISLANDS_LIGHT: Theme = Theme {
    dark: false,
    frame: hex(0xE9EAEE),
    island: hex(0xFFFFFF),
    popup: hex(0xFFFFFF),
    border: hex(0xC9CCD6),
    separator: hex(0xEBECF0),
    hover: hex(0xEDEEF2),
    pressed: hex(0xDFE1E5),
    selection: hex(0xD5E1FF),
    selection_inactive: hex(0xEBECF0),
    tool_open: hex(0xDFE1E5),
    accent: hex(0x3574F0),
    accent_hover: hex(0x3369D6),
    text: hex(0x000000),
    text_secondary: hex(0x818594),
    text_disabled: hex(0xA8ADBD),
    link: hex(0x2E55A3),
    line_number: hex(0xAEB3C2),
    line_number_current: hex(0x767A8A),
    caret_row: hex(0xF5F8FE),
    text_selection: hex(0xA6D2FF),
    field: hex(0xFFFFFF),
    modified: hex(0x0A7AFF),
    added: hex(0x3E8F4C),
    deleted: hex(0x818594),
    unversioned: hex(0xC7222D),
    conflict: hex(0xD5404A),
    diff_inserted: hex(0xE4F3E6),
    diff_deleted: hex(0xEBECF0),
    diff_modified: hex(0xE3EBFD),
    diff_empty: hex(0xF7F8FA),
    success: hex(0x208A3C),
    warning: hex(0xC27D04),
    error: hex(0xE55765),
};

pub fn theme_by_index(index: usize) -> Theme {
    match index {
        1 => DARK,
        2 => ISLANDS_LIGHT,
        _ => ISLANDS_DARK,
    }
}

fn theme_id() -> egui::Id {
    egui::Id::new("atelier_theme")
}

/// The theme the interface is drawn with.
pub fn theme(ctx: &egui::Context) -> Theme {
    ctx.data(|d| d.get_temp(theme_id())).unwrap_or(ISLANDS_DARK)
}

/// Family of Inter SemiBold, for titles.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "Inter",
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            "Inter SemiBold",
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
        (
            "JetBrains Mono",
            &include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's own fonts stay as fallbacks for symbols and emoji.
    let proportional = fonts.families[&FontFamily::Proportional].clone();
    let monospace = fonts.families[&FontFamily::Monospace].clone();
    let with = |first: &str, rest: &[String]| {
        std::iter::once(first.to_string())
            .chain(rest.iter().cloned())
            .collect::<Vec<_>>()
    };
    fonts
        .families
        .insert(FontFamily::Proportional, with("Inter", &proportional));
    fonts.families.insert(
        FontFamily::Name("semibold".into()),
        with("Inter SemiBold", &proportional),
    );
    let mut mono = with("JetBrains Mono", &monospace);
    mono.extend(proportional);
    fonts.families.insert(FontFamily::Monospace, mono);
    ctx.set_fonts(fonts);
}

/// Applies a theme to egui. Fonts are installed on first use.
pub fn install(ctx: &egui::Context, t: Theme, editor_font_size: f32) {
    let fonts_id = egui::Id::new("atelier_fonts");
    if !ctx.data(|d| d.get_temp::<bool>(fonts_id).unwrap_or(false)) {
        install_fonts(ctx);
        ctx.data_mut(|d| d.insert_temp(fonts_id, true));
    }
    ctx.data_mut(|d| d.insert_temp(theme_id(), t));
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = t.island;
    v.window_fill = t.popup;
    v.window_stroke = Stroke::new(1.0_f32, t.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 22,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 110 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 3],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 90 } else { 30 }),
    };
    v.extreme_bg_color = t.field;
    v.text_edit_bg_color = Some(t.field);
    v.faint_bg_color = t.hover;
    v.code_bg_color = t.frame;
    v.hyperlink_color = t.link;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.error;
    v.window_highlight_topmost = false;
    v.selection.bg_fill = t.text_selection;
    v.selection.stroke = Stroke::new(1.0_f32, t.accent);
    v.text_cursor.stroke = Stroke::new(2.0_f32, t.text);
    let radius = CornerRadius::same(4);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.island;
    w.noninteractive.weak_bg_fill = t.island;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text);
    w.inactive.bg_fill = t.field;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, t.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, t.text);
    w.hovered.bg_fill = t.hover;
    w.hovered.weak_bg_fill = t.hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, t.border);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, t.text);
    w.hovered.expansion = 0.0;
    w.active.bg_fill = t.pressed;
    w.active.weak_bg_fill = t.pressed;
    w.active.bg_stroke = Stroke::new(1.0_f32, t.accent);
    w.active.fg_stroke = Stroke::new(1.0_f32, t.text);
    w.active.expansion = 0.0;
    w.open.bg_fill = t.hover;
    w.open.weak_bg_fill = t.hover;
    w.open.bg_stroke = Stroke::new(1.0_f32, t.border);
    w.open.fg_stroke = Stroke::new(1.0_f32, t.text);
    for widget in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        widget.corner_radius = radius;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|style| {
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, semibold(15.0)),
            (TextStyle::Monospace, FontId::monospace(editor_font_size)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = vec2(8.0, 6.0);
        s.button_padding = vec2(10.0, 4.0);
        s.interact_size = vec2(36.0, 24.0);
        s.menu_margin = egui::Margin::same(6);
        s.window_margin = egui::Margin::same(14);
        s.indent = 18.0;
        s.icon_width = 15.0;
        s.icon_spacing = 6.0;
        s.combo_height = 300.0;
        s.scroll = egui::style::ScrollStyle {
            bar_width: 8.0,
            floating_width: 4.0,
            floating_allocated_width: 0.0,
            ..egui::style::ScrollStyle::floating()
        };
        style.interaction.selectable_labels = false;
    });
}

// ---- Icons ------------------------------------------------------------------------------

/// An IntelliJ icon for the current theme. Unknown names fall back to a neutral dot.
/// An icon of the interface or of a plugin (see `icon_set`) for the theme. Unknown names fall
/// back to a neutral dot.
pub fn icon_source(name: &str, dark: bool) -> ImageSource<'static> {
    crate::icon_set::source(name, dark)
        .or_else(|| crate::icon_set::source("moreHorizontal", dark))
        .expect("embedded icon")
}

/// The icon redrawn in white, for icons placed on an accent background.
pub fn icon_on_accent(name: &str) -> ImageSource<'static> {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static [u8]>>> = OnceLock::new();
    let ImageSource::Bytes {
        bytes: Bytes::Static(bytes),
        ..
    } = icon_source(name, true)
    else {
        return icon_source(name, true);
    };
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .expect("icon cache");
    let white = *cache.entry(name.to_string()).or_insert_with(|| {
        let svg = String::from_utf8_lossy(bytes)
            .replace("#CED0D6", "#FFFFFF")
            .replace("#DFE1E5", "#FFFFFF");
        Box::leak(svg.into_bytes().into_boxed_slice())
    });
    ImageSource::Bytes {
        uri: Cow::Owned(format!("bytes://ui-white/{name}.svg")),
        bytes: Bytes::Static(white),
    }
}

/// A 16 px icon image for the current theme.
pub fn icon(ui: &Ui, name: &str) -> egui::Image<'static> {
    egui::Image::new(icon_source(name, theme(ui.ctx()).dark)).fit_to_exact_size(vec2(16.0, 16.0))
}

pub fn paint_icon(ui: &Ui, name: &str, rect: Rect) {
    icon(ui, name).paint_at(ui, rect);
}

// ---- Widgets ----------------------------------------------------------------------------

/// Background of a hovered or pressed flat control.
pub fn paint_hover(ui: &Ui, rect: Rect, response: &Response, selected: bool) {
    let t = theme(ui.ctx());
    let fill = if response.is_pointer_button_down_on() {
        t.pressed
    } else if selected {
        t.tool_open
    } else if response.hovered() {
        t.hover
    } else {
        return;
    };
    ui.painter().rect_filled(rect, 5.0, fill);
}

/// Toolbar button: a 16 px icon, a rounded background on hover, a tooltip.
pub fn icon_button(ui: &mut Ui, name: &str, tooltip: &str) -> Response {
    icon_toggle(ui, name, false, tooltip)
}

pub fn icon_toggle(ui: &mut Ui, name: &str, selected: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());
    paint_hover(ui, rect, &response, selected);
    paint_icon(
        ui,
        name,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
    );
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// Disabled-looking icon button that cannot be clicked.
pub fn icon_button_enabled(ui: &mut Ui, name: &str, enabled: bool, tooltip: &str) -> Response {
    ui.add_enabled_ui(enabled, |ui| icon_button(ui, name, tooltip))
        .inner
}

/// Flat button with an icon and a label, as in tool window toolbars and title bar widgets.
pub fn icon_text_button(ui: &mut Ui, name: Option<&str>, text: &str, chevron: bool) -> Response {
    let t = theme(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(13.0), t.text);
    let icon_w = if name.is_some() { 22.0 } else { 0.0 };
    let chevron_w = if chevron { 18.0 } else { 0.0 };
    let size = vec2(16.0 + icon_w + galley.size().x + chevron_w, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    paint_hover(ui, rect, &response, false);
    let mut x = rect.left() + 8.0;
    if let Some(name) = name {
        paint_icon(
            ui,
            name,
            Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(16.0)),
        );
        x += icon_w;
    }
    ui.painter().galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley.clone(),
        t.text,
    );
    if chevron {
        let cx = x + galley.size().x + 4.0;
        paint_icon(
            ui,
            "chevronDown",
            Rect::from_min_size(pos2(cx, rect.center().y - 8.0), Vec2::splat(16.0)),
        );
    }
    response
}

fn text_button(ui: &mut Ui, text: &str, primary: bool) -> Response {
    let t = theme(ui.ctx());
    let color = if primary { Color32::WHITE } else { t.text };
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(13.0), color);
    let size = vec2((galley.size().x + 28.0).max(72.0), 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let painter = ui.painter();
    if primary {
        let fill = if response.hovered() && enabled {
            t.accent_hover
        } else {
            t.accent
        };
        painter.rect_filled(
            rect,
            5.0,
            if enabled {
                fill
            } else {
                fill.gamma_multiply(0.5)
            },
        );
    } else {
        let fill = if response.hovered() && enabled {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        painter.rect(
            rect,
            5.0,
            fill,
            Stroke::new(1.0_f32, t.border),
            StrokeKind::Inside,
        );
    }
    let color = if enabled { color } else { t.text_disabled };
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// Blue button of the main action of a dialog or a tool window.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    text_button(ui, text, true)
}

/// Outlined button.
pub fn button(ui: &mut Ui, text: &str) -> Response {
    text_button(ui, text, false)
}

/// IntelliJ checkbox: blue when checked. `None` draws the indeterminate state.
pub fn check_state(ui: &mut Ui, state: Option<bool>, text: &str) -> Response {
    let t = theme(ui.ctx());
    let galley = (!text.is_empty()).then(|| {
        ui.painter()
            .layout_no_wrap(text.into(), FontId::proportional(13.0), t.text)
    });
    let width = 16.0 + galley.as_ref().map_or(0.0, |g| g.size().x + 8.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    let boxed = Rect::from_min_size(pos2(rect.left(), rect.center().y - 8.0), Vec2::splat(16.0));
    let painter = ui.painter();
    let on = state != Some(false);
    if on {
        painter.rect_filled(
            boxed,
            4.0,
            if response.hovered() {
                t.accent_hover
            } else {
                t.accent
            },
        );
        let stroke = Stroke::new(1.6_f32, Color32::WHITE);
        if state.is_none() {
            painter.line_segment(
                [
                    boxed.center() - vec2(4.0, 0.0),
                    boxed.center() + vec2(4.0, 0.0),
                ],
                stroke,
            );
        } else {
            let c = boxed.center();
            painter.line_segment([c + vec2(-4.0, 0.0), c + vec2(-1.2, 3.0)], stroke);
            painter.line_segment([c + vec2(-1.2, 3.0), c + vec2(4.2, -3.2)], stroke);
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
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
    }
    if let Some(galley) = galley {
        painter.galley(
            pos2(boxed.right() + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            t.text,
        );
    }
    response
}

pub fn checkbox(ui: &mut Ui, value: &mut bool, text: &str) -> Response {
    let mut response = check_state(ui, Some(*value), text);
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response
}

/// Radio button with the IntelliJ look.
pub fn radio(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let t = theme(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(13.0), t.text);
    let (rect, response) =
        ui.allocate_exact_size(vec2(24.0 + galley.size().x, 22.0), Sense::click());
    let center = pos2(rect.left() + 8.0, rect.center().y);
    let painter = ui.painter();
    if selected {
        painter.circle_filled(center, 8.0, t.accent);
        painter.circle_filled(center, 3.0, Color32::WHITE);
    } else {
        let border = if response.hovered() {
            t.accent
        } else {
            t.border
        };
        painter.circle(center, 7.5, t.field, Stroke::new(1.0_f32, border));
    }
    painter.galley(
        pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    response
}

pub fn radio_value<T: PartialEq>(ui: &mut Ui, current: &mut T, value: T, text: &str) -> Response {
    let mut response = radio(ui, *current == value, text);
    if response.clicked() && *current != value {
        *current = value;
        response.mark_changed();
    }
    response
}

/// Rounded search field with a magnifier, as in IntelliJ popups and tool windows.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let t = theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let edit = egui::TextEdit::singleline(text)
        .hint_text(hint)
        .frame(false)
        .vertical_align(egui::Align::Center)
        .margin(egui::Margin {
            left: 28,
            right: 8,
            top: 0,
            bottom: 0,
        });
    let response = ui.put(rect, edit);
    let border = if response.has_focus() {
        t.accent
    } else {
        t.border
    };
    ui.painter()
        .rect_stroke(rect, 5.0, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    paint_icon(
        ui,
        "search",
        Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.center().y - 8.0),
            Vec2::splat(16.0),
        ),
    );
    response
}

/// Chevron of a tree node, 16 px, pointing right or down.
pub fn paint_chevron(ui: &Ui, center: Pos2, open: bool) {
    let name = if open { "chevronDown" } else { "chevronRight" };
    paint_icon(ui, name, Rect::from_center_size(center, Vec2::splat(16.0)));
}

/// A full-width row of a tree or list. Paints the hover and selection background.
pub fn row(ui: &mut Ui, selected: bool) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    paint_row(ui, rect, selected, response.hovered());
    (rect, response)
}

pub fn paint_row(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    let t = theme(ui.ctx());
    let fill = if selected {
        t.selection
    } else if hovered {
        t.hover
    } else {
        return;
    };
    ui.painter()
        .rect_filled(rect.shrink2(vec2(4.0, 0.0)), 4.0, fill);
}

/// Text drawn left-aligned and vertically centered in a row; returns its right edge.
pub fn paint_text(ui: &Ui, rect: Rect, x: f32, text: &str, color: Color32, font: FontId) -> f32 {
    let galley = ui.painter().layout_no_wrap(text.into(), font, color);
    let width = galley.size().x;
    ui.painter().with_clip_rect(rect).galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    x + width
}

/// Rounded label, e.g. a branch or tag on a commit.
pub fn paint_badge(
    ui: &Ui,
    painter: &egui::Painter,
    x: f32,
    center_y: f32,
    text: &str,
    color: Color32,
) -> f32 {
    let galley = painter.layout_no_wrap(text.into(), FontId::proportional(11.5), color);
    let rect = Rect::from_min_size(
        pos2(x, center_y - galley.size().y / 2.0 - 2.0),
        galley.size() + vec2(10.0, 4.0),
    );
    let _ = ui;
    painter.rect(
        rect,
        4.0,
        color.gamma_multiply(0.14),
        Stroke::new(1.0_f32, color.gamma_multiply(0.5)),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(5.0, 2.0), galley, color);
    rect.right()
}

/// Small secondary title inside a tool window or dialog.
pub fn section(ui: &mut Ui, text: &str) {
    let t = theme(ui.ctx());
    ui.label(
        RichText::new(text)
            .font(semibold(12.0))
            .color(t.text_secondary),
    );
}

/// Title row of a dialog.
pub fn dialog_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(15.0)));
    ui.add_space(6.0);
}

/// Buttons of a dialog, right-aligned, the primary one last as in IntelliJ on Windows.
/// Enter confirms and Escape cancels. Returns (confirmed, cancelled).
pub fn dialog_buttons(ui: &mut Ui, primary: &str) -> (bool, bool) {
    ui.add_space(10.0);
    let mut ok = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        cancel |= button(ui, "Annuler").clicked();
        ok |= primary_button(ui, primary).clicked();
    });
    (ok, cancel)
}

// ---- Menus --------------------------------------------------------------------------------

/// Width menus open at, and height of their entries.
pub const MENU_WIDTH: f32 = 230.0;
const MENU_ROW: f32 = 28.0;

/// Call first in a context menu or popup: sets the width and tightens the spacing.
pub fn menu_begin(ui: &mut Ui) {
    ui.set_min_width(MENU_WIDTH);
    ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
}

/// A menu entry: optional icon, label, optional shortcut on the right. `danger` colors it as
/// an error (delete…). Closes the menu on click, as the caller reads `clicked()`.
pub fn menu_item(ui: &mut Ui, icon: Option<&str>, label: &str, shortcut: Option<&str>, danger: bool) -> Response {
    menu_entry(ui, icon, label, shortcut, danger, true)
}

pub fn menu_entry(
    ui: &mut Ui,
    icon: Option<&str>,
    label: &str,
    shortcut: Option<&str>,
    danger: bool,
    enabled: bool,
) -> Response {
    let t = theme(ui.ctx());
    let width = ui.available_width().max(MENU_WIDTH - 12.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, MENU_ROW), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.selection);
    }
    let color = match (enabled, danger) {
        (false, _) => t.text_secondary.gamma_multiply(0.6),
        (true, true) => t.error,
        (true, false) => t.text,
    };
    if let Some(icon) = icon {
        let slot = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0));
        let image = self::icon(ui, icon);
        if enabled {
            image.paint_at(ui, slot);
        } else {
            image.tint(Color32::from_white_alpha(100)).paint_at(ui, slot);
        }
    }
    ui.painter().text(
        pos2(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        color,
    );
    if let Some(shortcut) = shortcut {
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::proportional(12.0),
            t.text_secondary,
        );
    }
    response
}

/// Thin line between groups of entries.
pub fn menu_separator(ui: &mut Ui) {
    let t = theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        rect.left() + 8.0..=rect.right() - 8.0,
        rect.center().y,
        Stroke::new(1.0_f32, t.separator),
    );
}

/// Small grey title of a group of entries.
pub fn menu_title(ui: &mut Ui, text: &str) {
    let t = theme(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y + 2.0),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(11.5),
        t.text_secondary,
    );
}

/// Text with a vertical separator before, as between status bar widgets.
pub fn status_text(ui: &mut Ui, text: &str) -> Response {
    let t = theme(ui.ctx());
    let galley =
        ui.painter()
            .layout_no_wrap(text.into(), FontId::proportional(12.5), t.text_secondary);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 14.0, 24.0), Sense::click());
    paint_hover(ui, rect, &response, false);
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        t.text_secondary,
    );
    response
}

/// Status bar text of a fixed width, the one `widest` needs, aligned right: a value that changes
/// every second (memory, processor) must not push its neighbours around.
pub fn status_text_fixed(ui: &mut Ui, text: &str, widest: &str) -> Response {
    let t = theme(ui.ctx());
    let font = FontId::proportional(12.5);
    let width = ui.painter().layout_no_wrap(widest.into(), font.clone(), t.text_secondary).size().x;
    let galley = ui.painter().layout_no_wrap(text.into(), font, t.text_secondary);
    let (rect, response) = ui.allocate_exact_size(vec2(width.max(galley.size().x) + 14.0, 24.0), Sense::hover());
    ui.painter().galley(egui::pos2(rect.right() - 7.0 - galley.size().x, rect.center().y - galley.size().y / 2.0), galley, t.text_secondary);
    response
}

/// Paints `text` centered in `rect`, in the secondary color.
pub fn paint_centered(ui: &Ui, rect: Rect, text: &str) {
    let t = theme(ui.ctx());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        t.text_secondary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_the_theme_and_plugins_keep_their_own() {
        for name in ["git", "branch", "commit", "push", "vcsCommit", "update"] {
            let (light, dark) = (icon_source(name, false), icon_source(name, true));
            assert_ne!(light.uri(), dark.uri(), "{name}: one file per theme");
        }
        // Only a plain file: both themes share it.
        let plain = crate::icon_set::find("msoffice/icon").unwrap();
        assert!(plain.variants[1].is_none() && plain.variants[2].is_none());
        assert_eq!(
            crate::icon_set::source("msoffice/icon", true).unwrap().uri(),
            crate::icon_set::source("msoffice/icon", false).unwrap().uri()
        );
        // A name that exists nowhere falls back to a dot instead of failing.
        assert!(icon_source("nope", true).uri().unwrap().contains("moreHorizontal"));
        assert!(icon_on_accent("project").uri().unwrap().contains("ui-white"));
    }

    #[test]
    fn themes_install_and_fonts_resolve() {
        let ctx = egui::Context::default();
        for index in 0..THEMES.len() {
            install(&ctx, theme_by_index(index), 14.0);
            assert_eq!(theme(&ctx), theme_by_index(index));
        }
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(RichText::new("Projet").font(semibold(13.0)));
                ui.monospace("fn main() {}");
            });
        });
    }
}
