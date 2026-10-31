//! Mermaid built-in plugin: `.mmd` files get an editor, a rendered diagram, or both side by side,
//! with export to PNG and SVG. The Markdown preview uses [`render`] and [`split_blocks`] to draw
//! the ```mermaid blocks of a document.
//!
//! Drawing a diagram can take long, and a huge one must not take the window down with it: it is
//! laid out and rasterized on a thread of its own with a big stack, shown as a background task
//! in the status bar, and the interface draws a spinner until the image is there. Exports run
//! the same way, and refuse an image that would not fit in memory.

use crate::{
    plugins::builtin::{BuiltinPlugin, Command, EditorLayout, Host},
    ui,
};
use eframe::egui::{self, Margin};
use mermaid_rs_renderer::{RenderConfig, RenderOptions, Theme};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// A source longer than this is not drawn: the layout would run for minutes.
const MAX_SOURCE: usize = 400_000;
/// The picture shown is rasterized once, at most this many pixels on its long side (the GPU
/// texture limit), and at most this many times the natural size.
const MAX_TEXTURE: f32 = 2048.0;
const MAX_UPSCALE: f32 = 3.0;
/// A PNG export above this many pixels would need more memory than it is worth: use the SVG.
const MAX_EXPORT_PIXELS: f64 = 60_000_000.0;
/// The layout runs deep recursions on big graphs.
const WORKER_STACK: usize = 256 << 20;
/// After this long a diagram is given up on, the interface says so instead of waiting forever.
const GIVE_UP: Duration = Duration::from_secs(180);

/// A diagram rendered to SVG, or why it could not be.
pub type Rendered = Result<Diagram, String>;

/// The SVG of a diagram and its natural size, read from the `viewBox`: the SVG itself only
/// gives relative sizes, which would leave the image loader to guess.
#[derive(Clone)]
pub struct Diagram {
    pub svg: Arc<[u8]>,
    /// What is drawn: the diagram rasterized once, within the texture limit.
    pub png: Arc<[u8]>,
    pub size: egui::Vec2,
}

fn view_box_size(svg: &str) -> Option<egui::Vec2> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let values: Vec<f32> = svg[start..]
        .split('"')
        .next()?
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    match values[..] {
        [_, _, w, h] if w > 0.0 && h > 0.0 => Some(egui::vec2(w, h)),
        _ => None,
    }
}

impl Diagram {
    /// Draws the diagram scaled down to fit `max` (never up, unless `fill`), within the
    /// texture limit.
    pub fn image(&self, source: &str, max: egui::Vec2, fill: bool) -> egui::Image<'static> {
        let max = max
            .max(egui::vec2(50.0, 50.0))
            .min(egui::vec2(2048.0, 2048.0));
        let mut scale = (max.x / self.size.x).min(max.y / self.size.y);
        if !fill {
            scale = scale.min(1.0);
        }
        egui::Image::from_bytes(uri(source), self.png.to_vec()).fit_to_exact_size(self.size * scale)
    }
}

pub fn is_mermaid(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mmd" | "mermaid"))
}

pub fn hash(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// Image URI of a diagram: the same source always gives the same one.
pub fn uri(source: &str) -> String {
    format!("bytes://mermaid/{:x}.png", hash(source))
}

/// The picture of a diagram: its SVG rasterized within the texture limit, as a PNG.
fn rasterize(svg: &str) -> Result<(Vec<u8>, egui::Vec2), String> {
    use resvg::{tiny_skia, usvg};
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let natural = tree.size();
    let long = natural.width().max(natural.height()).max(1.0);
    let scale = (MAX_TEXTURE / long).min(MAX_UPSCALE);
    let (width, height) = (
        (natural.width() * scale).ceil().max(1.0) as u32,
        (natural.height() * scale).ceil().max(1.0) as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(tr!("mermaid.image_too_large"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(
            width as f32 / natural.width(),
            height as f32 / natural.height(),
        ),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().map_err(|e| e.to_string())?;
    Ok((png, egui::vec2(natural.width(), natural.height())))
}

/// Draws a diagram now, on this thread. The interface goes through [`render_async`].
pub fn render_blocking(source: &str) -> Rendered {
    if source.len() > MAX_SOURCE {
        return Err(trf!(
            "mermaid.diagram_too_large_kb_at",
            source = source.len() / 1000,
            value = MAX_SOURCE / 1000
        ));
    }
    catch_unwind(AssertUnwindSafe(|| {
        let svg = mermaid_rs_renderer::render_with_options(source, RenderOptions::modern())
            .map_err(|e| e.to_string())?;
        let (png, size) = rasterize(&svg)?;
        Ok(Diagram {
            size: view_box_size(&svg).unwrap_or(size),
            svg: Arc::from(svg.into_bytes()),
            png: Arc::from(png),
        })
    }))
    .unwrap_or_else(|_| Err(tr!("mermaid.diagram_not_supported").to_string()))
}

enum Slot {
    Drawing(Instant),
    Done(Rendered),
}

/// The diagrams drawn or being drawn, by source.
static JOBS: Mutex<Option<HashMap<u64, Slot>>> = Mutex::new(None);

/// The diagram of `source` when it is drawn. The first call starts drawing it on a thread of
/// its own, which shows in the status bar, and returns None until it is done (the window is
/// repainted when it is).
pub fn render_async(source: &str, ctx: &egui::Context) -> Option<Rendered> {
    let key = hash(source);
    let mut guard = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    let jobs = guard.get_or_insert_with(HashMap::new);
    match jobs.get(&key) {
        Some(Slot::Done(result)) => return Some(result.clone()),
        Some(Slot::Drawing(since)) if since.elapsed() > GIVE_UP => {
            return Some(Err(trf!(
                "mermaid.timed_out_s_diagram_too",
                as_secs = GIVE_UP.as_secs()
            )));
        }
        Some(Slot::Drawing(_)) => return None,
        None => {}
    }
    // Finished results are kept for the next frames, a few at a time.
    if jobs.len() >= 64 {
        jobs.retain(|_, slot| matches!(slot, Slot::Drawing(_)));
    }
    jobs.insert(key, Slot::Drawing(Instant::now()));
    let (source, ctx) = (source.to_string(), ctx.clone());
    // Announced before the thread starts, so that it is listed from the first frame on.
    let task = crate::tasks::start(tr!("mermaid.drawing_mermaid_diagram"));
    let spawned = std::thread::Builder::new()
        .name("mermaid".into())
        .stack_size(WORKER_STACK)
        .spawn(move || {
            let _task = task;
            let result = render_blocking(&source);
            JOBS.lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(key, Slot::Done(result));
            ctx.request_repaint();
        });
    if spawned.is_err() {
        jobs.insert(
            key,
            Slot::Done(Err(tr!("mermaid.cannot_start_drawing_diagram").into())),
        );
        return jobs.get(&key).and_then(|slot| {
            if let Slot::Done(r) = slot {
                Some(r.clone())
            } else {
                None
            }
        });
    }
    None
}

/// What stands in the place of a diagram that is being drawn.
fn drawing(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(tr!("mermaid.drawing_diagram"));
    });
    ui.ctx().request_repaint_after(Duration::from_millis(100));
}

/// Why a diagram cannot be exported as a PNG, when it cannot.
fn export_refusal(size: egui::Vec2) -> Option<String> {
    let pixels = size.x as f64 * size.y as f64;
    (pixels > MAX_EXPORT_PIXELS).then(|| {
        trf!(
            "mermaid.image_px_too_large_png",
            x = format!("{:.0}", size.x),
            y = format!("{:.0}", size.y)
        )
    })
}

/// Writes the diagram as a PNG image.
pub fn export_png(diagram: &Diagram, path: &Path) -> Result<(), String> {
    if let Some(reason) = export_refusal(diagram.size) {
        return Err(reason);
    }
    let svg = String::from_utf8_lossy(&diagram.svg);
    catch_unwind(AssertUnwindSafe(|| {
        mermaid_rs_renderer::write_output_png(
            &svg,
            path,
            &RenderConfig::default(),
            &Theme::modern(),
        )
    }))
    .map_err(|_| tr!("mermaid.cannot_export_2").to_string())?
    .map_err(|e| e.to_string())
}

pub fn export_svg(diagram: &Diagram, path: &Path) -> Result<(), String> {
    std::fs::write(path, &*diagram.svg).map_err(|e| e.to_string())
}

/// Runs an export on a thread of its own and leaves its result in `message`.
fn export_in_background(
    title: &'static str,
    diagram: Diagram,
    target: PathBuf,
    png: bool,
    message: Arc<Mutex<Option<(bool, String)>>>,
    ctx: egui::Context,
) {
    let task = crate::tasks::start(title);
    let spawned = std::thread::Builder::new()
        .name("mermaid-export".into())
        .stack_size(WORKER_STACK)
        .spawn(move || {
            let _task = task;
            let result = if png {
                export_png(&diagram, &target)
            } else {
                export_svg(&diagram, &target)
            };
            *message.lock().unwrap_or_else(|e| e.into_inner()) = Some(match result {
                Ok(()) => (false, trf!("mermaid.exported", target = target.display())),
                Err(e) => (true, trf!("mermaid.cannot_export", e = e)),
            });
            ctx.request_repaint();
        });
    let _ = spawned;
}

/// A piece of a Markdown document: text, or the source of a mermaid block.
#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    Text(String),
    Diagram(String),
}

/// Cuts a Markdown document around its ```mermaid (or ~~~mermaid) fences. Everything else,
/// including other code blocks, stays in the text pieces, unchanged.
pub fn split_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    // The fence being read: its character and length, and whether it holds a diagram.
    let mut fence: Option<(char, usize, bool)> = None;
    let mut diagram = String::new();
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed
            .chars()
            .next()
            .filter(|c| matches!(c, '`' | '~') && indent < 4);
        let run = marker.map_or(0, |m| trimmed.chars().take_while(|&c| c == m).count());
        match fence {
            None if run >= 3 => {
                let info = trimmed[run..].trim();
                let is_diagram = info.split_whitespace().next() == Some("mermaid");
                fence = Some((marker.unwrap_or('`'), run, is_diagram));
                if is_diagram {
                    if !current.is_empty() {
                        blocks.push(Block::Text(std::mem::take(&mut current)));
                    }
                } else {
                    current.push_str(line);
                }
            }
            Some((m, n, is_diagram)) => {
                let closes = marker == Some(m) && run >= n && trimmed[run..].trim().is_empty();
                if closes {
                    fence = None;
                    if is_diagram {
                        blocks.push(Block::Diagram(std::mem::take(&mut diagram)));
                    } else {
                        current.push_str(line);
                    }
                } else if is_diagram {
                    diagram.push_str(line);
                } else {
                    current.push_str(line);
                }
            }
            None => current.push_str(line),
        }
    }
    if let Some((_, _, true)) = fence {
        // Unclosed fence: still a diagram, as Markdown readers treat it.
        blocks.push(Block::Diagram(diagram));
    }
    if !current.is_empty() {
        blocks.push(Block::Text(current));
    }
    blocks
}

/// Draws a rendered diagram, or its error, at most `max_width` wide.
pub fn show(ui: &mut egui::Ui, source: &str, max_width: f32) {
    match render_async(source, ui.ctx()) {
        None => drawing(ui),
        Some(Ok(diagram)) => {
            ui.add(diagram.image(source, egui::vec2(max_width, 2048.0), false));
        }
        Some(Err(error)) => {
            let t = ui::theme(ui.ctx());
            ui.colored_label(
                t.error,
                trf!("mermaid.invalid_mermaid_diagram", error = error),
            );
        }
    }
}

#[derive(Default)]
pub struct MermaidPlugin {
    layouts: HashMap<PathBuf, EditorLayout>,
    /// Zoom of each preview, 1 meaning the diagram fits the pane.
    zoom: HashMap<PathBuf, f32>,
    /// Last image registered for each document, forgotten when the text changes.
    uris: HashMap<PathBuf, String>,
    /// Result of the last export, left by its thread and shown by the toolbar.
    message: Arc<Mutex<Option<(bool, String)>>>,
}

impl BuiltinPlugin for MermaidPlugin {
    fn file_closed(&mut self, ctx: &egui::Context, path: &Path) {
        self.zoom.remove(path);
        if let Some(uri) = self.uris.remove(path) {
            ctx.forget_image(&uri);
        }
    }
    fn id(&self) -> &'static str {
        "mermaid"
    }
    fn name(&self) -> &'static str {
        "Mermaid"
    }
    fn description(&self) -> &'static str {
        tr!("mermaid.mermaid_diagrams_mmd_files_editor")
    }
    fn icon(&self) -> &'static str {
        "editorPreview"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("mermaid")
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("mermaid.text", tr!("mermaid.mermaid_editor_only")),
            ("mermaid.split", tr!("mermaid.mermaid_editor_preview")),
            ("mermaid.preview", tr!("mermaid.mermaid_preview_only")),
        ]
        .into_iter()
        .map(|(id, label)| Command {
            id,
            label: label.into(),
        })
        .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        let layout = match id {
            "mermaid.text" => EditorLayout::Text,
            "mermaid.split" => EditorLayout::Split,
            "mermaid.preview" => EditorLayout::Preview,
            _ => return,
        };
        match host.active_file().filter(|p| is_mermaid(p)) {
            Some(path) => self.set_editor_layout(&path, layout),
            None => host.notify(tr!("mermaid.open_mermaid_file_mmd").into()),
        }
    }

    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_mermaid(path).then(|| self.layouts.get(path).copied().unwrap_or_default())
    }

    fn set_editor_layout(&mut self, path: &Path, layout: EditorLayout) {
        self.layouts.insert(path.to_path_buf(), layout);
    }

    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, _scroll: Option<f32>) {
        let t = ui::theme(ui.ctx());
        let rendered = render_async(text, ui.ctx());
        let zoom = self.zoom.entry(path.to_path_buf()).or_insert(1.0);

        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let ready = rendered.as_ref().and_then(|r| r.as_ref().ok()).cloned();
            for (label, title, png, filter, extension) in [
                (
                    tr!("mermaid.export_png"),
                    tr!("mermaid.export_diagram_as_png"),
                    true,
                    tr!("mermaid.png_image"),
                    "png",
                ),
                (
                    tr!("mermaid.export_svg"),
                    tr!("mermaid.export_diagram_as_svg"),
                    false,
                    tr!("mermaid.svg_image"),
                    "svg",
                ),
            ] {
                if ui
                    .add_enabled(ready.is_some(), egui::Button::new(label))
                    .clicked()
                    && let Some(diagram) = ready.clone()
                    && let Some(target) = rfd::FileDialog::new()
                        .set_file_name(stem(path, extension))
                        .add_filter(filter, &[extension])
                        .save_file()
                {
                    export_in_background(
                        title,
                        diagram,
                        target,
                        png,
                        self.message.clone(),
                        ui.ctx().clone(),
                    );
                }
            }
            ui.separator();
            if ui.button("−").clicked() {
                *zoom = (*zoom / 1.25).max(0.25);
            }
            if ui
                .button(format!("{:.0} %", *zoom * 100.0))
                .on_hover_text(tr!("mermaid.fit_window"))
                .clicked()
            {
                *zoom = 1.0;
            }
            if ui.button("+").clicked() {
                *zoom = (*zoom * 1.25).min(4.0);
            }
            if let Some((error, message)) = &*self.message.lock().unwrap_or_else(|e| e.into_inner())
            {
                ui.add_space(8.0);
                ui.colored_label(if *error { t.error } else { t.text_secondary }, message);
            }
        });
        ui.separator();

        let diagram = match rendered {
            None => {
                egui::Frame::new()
                    .inner_margin(Margin::same(18))
                    .show(ui, drawing);
                return;
            }
            Some(Ok(diagram)) => diagram,
            Some(Err(error)) => {
                egui::Frame::new()
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| {
                        ui.colored_label(t.error, trf!("mermaid.invalid_diagram", error = error));
                    });
                return;
            }
        };
        let uri = uri(text);
        if let Some(old) = self.uris.insert(path.to_path_buf(), uri.clone())
            && old != uri
        {
            ui.ctx().forget_image(&old);
        }
        // Fitted to the pane, then zoomed; the texture stays within the GPU limit.
        let fit = (ui.available_size() - egui::vec2(36.0, 36.0)).max(egui::vec2(100.0, 100.0));
        let max = fit * *zoom;
        egui::ScrollArea::both()
            .id_salt(("mermaid_preview", path))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| {
                        ui.add(diagram.image(text, max, true));
                    });
            });
    }
}

/// Suggested name of an export: the document name with the extension of the image.
fn stem(path: &Path, extension: &str) -> String {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("diagramme");
    format!("{name}.{extension}")
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/mermaid/mod/tests.rs"]
mod tests;
