//! The window of the bootstrap, like the updaters of Discord or VS Code: the logo, what is
//! happening (looking for an update, downloading it, installing it, starting) and a progress bar.
//! The work runs on a thread ([`super::check_and_start`]); the window closes once the IDE is
//! started, and stays open with the reason when it could not be.

use super::{Progress, check_and_start};
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Sense,
    TextureHandle, ViewportCommand, pos2, vec2,
};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const SIZE: [f32; 2] = [300.0, 340.0];
const BACKGROUND: Color32 = Color32::from_rgb(0x1E, 0x1F, 0x22);
const TEXT: Color32 = Color32::from_rgb(0xDF, 0xE1, 0xE5);
const MUTED: Color32 = Color32::from_rgb(0x9D, 0xA0, 0xA8);
const TRACK: Color32 = Color32::from_rgb(0x39, 0x3B, 0x40);
/// The violet and the blue of the logo.
const START: Color32 = Color32::from_rgb(0xB4, 0x3C, 0xF0);
const END: Color32 = Color32::from_rgb(0x35, 0x74, 0xF0);
const LOGO: &[u8] = include_bytes!("../../../assets/logo/atelier.png");
/// Long enough for the window of the IDE to show up before this one goes.
const LINGER: Duration = Duration::from_millis(400);
/// Seconds the window takes to fade out once the IDE is started.
const FADE: f64 = 0.2;

/// Shows the window, checks for an update, installs it, starts the IDE with `args`.
pub fn run(dir: PathBuf, args: Vec<OsString>) {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Atelier IDE")
        .with_app_id("atelier")
        .with_inner_size(SIZE)
        .with_resizable(false)
        .with_decorations(false)
        // For the rounded corners and the fade out: what the window does not paint is see-through.
        .with_transparent(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(LOGO) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    let started = eframe::run_native(
        "atelier",
        options,
        Box::new(|cc| {
            Ok(Box::new(Splash::new(
                &cc.egui_ctx,
                dir.clone(),
                args.clone(),
            )))
        }),
    );
    // Without a window (no graphics), the work is done all the same.
    if started.is_err()
        && let Progress::Failed(message) = check_and_start(&dir, args, true, &|_| {})
    {
        super::fail(&message);
    }
}

struct Splash {
    dir: PathBuf,
    args: Vec<OsString>,
    progress: Arc<Mutex<Progress>>,
    /// When the fade out began.
    closing: Option<f64>,
    logo: Option<TextureHandle>,
}

impl Splash {
    fn new(ctx: &egui::Context, dir: PathBuf, args: Vec<OsString>) -> Self {
        fonts(ctx);
        let logo = image::load_from_memory(LOGO).ok().map(|image| {
            let image = image.to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            ctx.load_texture("logo", pixels, egui::TextureOptions::LINEAR)
        });
        let splash = Self {
            dir,
            args,
            progress: Arc::new(Mutex::new(Progress::Checking)),
            closing: None,
            logo,
        };
        splash.start(ctx);
        splash
    }

    /// Runs the check and the start of the IDE on a thread.
    fn start(&self, ctx: &egui::Context) {
        self.set(Progress::Checking);
        let (dir, args) = (self.dir.clone(), self.args.clone());
        let (progress, ctx) = (self.progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let report = |step: Progress| {
                *progress.lock().unwrap() = step;
                ctx.request_repaint();
            };
            let end = check_and_start(&dir, args, true, &report);
            if end == Progress::Done {
                std::thread::sleep(LINGER);
            }
            report(end);
        });
    }

    fn set(&self, step: Progress) {
        *self.progress.lock().unwrap() = step;
    }
}

impl eframe::App for Splash {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let step = self.progress.lock().unwrap().clone();
        // Once the IDE is started, the window fades out, then closes: already invisible, it does
        // not show the animation Windows plays when a window closes.
        let mut opacity = 1.0;
        if step == Progress::Done {
            let now = ctx.input(|i| i.time);
            let since = now - *self.closing.get_or_insert(now);
            if since >= FADE {
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return;
            }
            opacity = 1.0 - (since / FADE) as f32;
            ctx.request_repaint();
        }
        let frame = egui::Frame::new()
            .fill(BACKGROUND.gamma_multiply(opacity))
            .corner_radius(12)
            .inner_margin(24);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.set_opacity(opacity);
            // No title bar: the whole window moves it.
            let whole = ui.interact(ui.max_rect(), ui.id().with("drag"), Sense::drag());
            if whole.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            // An error needs the room of the message: the logo gets smaller.
            let failed = matches!(step, Progress::Failed(_));
            let (space, side) = if failed { (0.0, 64.0) } else { (20.0, 112.0) };
            ui.vertical_centered(|ui| {
                ui.add_space(space);
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(side, side)));
                }
                ui.add_space(if failed { 12.0 } else { 20.0 });
                ui.label(
                    egui::RichText::new("Atelier IDE")
                        .font(FontId::new(19.0, FontFamily::Name("semibold".into())))
                        .color(TEXT),
                );
                ui.add_space(10.0);
                if let Progress::Failed(message) = &step {
                    self.failed(ui, message);
                } else {
                    ui.label(egui::RichText::new(status(&step)).size(13.0).color(MUTED));
                    ui.add_space(16.0);
                    bar(ui, fraction(&step));
                }
            });
        });
        // The bar moves on its own while nothing is known of the progress.
        if fraction(&step).is_none() {
            ctx.request_repaint();
        }
    }
}

impl Splash {
    fn failed(&self, ui: &mut egui::Ui, message: &str) {
        egui::ScrollArea::vertical()
            .max_height(120.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(message).size(12.0).color(MUTED));
            });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let width = 90.0 * 2.0 + ui.spacing().item_spacing.x;
            ui.add_space((ui.available_width() - width) / 2.0);
            if ui
                .add_sized([90.0, 28.0], egui::Button::new(tr!("retry")))
                .clicked()
            {
                self.start(ui.ctx());
            }
            if ui
                .add_sized([90.0, 28.0], egui::Button::new(tr!("close")))
                .clicked()
            {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
        });
    }
}

/// The line under the name.
fn status(step: &Progress) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    match step {
        Progress::Checking => tr!("splash_checking").to_string(),
        Progress::Downloading { tag, done, total } => trf!(
            "splash_downloading",
            tag = tag,
            done = format!("{:.1}", *done as f64 / MB),
            total = format!("{:.1}", *total as f64 / MB),
        ),
        Progress::Installing { tag } => trf!("splash_installing", tag = tag),
        Progress::Starting | Progress::Done => tr!("splash_starting").to_string(),
        Progress::Failed(message) => message.clone(),
    }
}

/// How far the bar is filled, or `None` when nothing tells how long it will take.
fn fraction(step: &Progress) -> Option<f32> {
    match step {
        Progress::Downloading { done, total, .. } if *total > 0 => {
            Some((*done as f32 / *total as f32).clamp(0.0, 1.0))
        }
        Progress::Failed(_) => Some(0.0),
        _ => None,
    }
}

/// A thin bar in the colors of the logo. Without a fraction, a segment goes back and forth.
fn bar(ui: &mut egui::Ui, fraction: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(200.0, 4.0), Sense::hover());
    let painter = ui.painter();
    let radius = CornerRadius::same(2);
    painter.rect_filled(rect, radius, TRACK);
    let (from, to) = match fraction {
        Some(f) => (0.0, f),
        None => {
            let t = ui.input(|i| i.time) as f32;
            // 0 → 1 → 0 in 1.6 s, eased.
            let phase = (t / 1.6).fract();
            let x = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
            let start = x * 0.7;
            (start, start + 0.3)
        }
    };
    if to <= from {
        return;
    }
    let fill = Rect::from_min_max(
        pos2(rect.left() + rect.width() * from, rect.top()),
        pos2(rect.left() + rect.width() * to, rect.bottom()),
    );
    painter.rect_filled(fill, radius, START.lerp_to_gamma(END, (from + to) / 2.0));
}

/// Source Sans 3, the font of the IDE.
fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "source-sans",
            &include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf")[..],
        ),
        (
            "source-sans-semibold",
            &include_bytes!("../../../assets/fonts/SourceSans3-Semibold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "source-sans".into());
    let mut semibold = vec!["source-sans-semibold".to_string()];
    semibold.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold);
    ctx.set_fonts(fonts);
}
