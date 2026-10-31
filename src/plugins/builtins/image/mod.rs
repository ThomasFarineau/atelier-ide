//! Image viewer built-in plugin: PNG, JPEG, GIF, WebP, BMP and ICO files open in a viewer
//! instead of the text editor.

use crate::{
    plugins::builtin::{BuiltinPlugin, EditorLayout},
    ui,
};
use eframe::egui::{self, ColorImage, Margin, RichText, TextureHandle, TextureOptions};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Largest image read, in bytes.
const MAX_SIZE: u64 = 64 * 1024 * 1024;

struct Loaded {
    modified: Option<SystemTime>,
    /// Texture and original size in pixels.
    texture: Result<(TextureHandle, [u32; 2]), String>,
}

#[derive(Default)]
pub struct ImagePlugin {
    loaded: HashMap<PathBuf, Loaded>,
}

pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
        )
    })
}

/// Largest side of the texture sent to the GPU; bigger images are shrunk.
const MAX_SIDE: u32 = 2048;

fn load(ctx: &egui::Context, path: &Path) -> Result<(TextureHandle, [u32; 2]), String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_SIZE {
        return Err(tr!("image.image_too_large_limit_64").into());
    }
    let image = image::open(path).map_err(|e| trf!("image.unreadable_image", e = e))?;
    let original = [image.width(), image.height()];
    let image = if original.iter().any(|&side| side > MAX_SIDE) {
        image.thumbnail(MAX_SIDE, MAX_SIDE)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let color = ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Ok((
        ctx.load_texture(
            format!("image:{}", path.display()),
            color,
            TextureOptions::LINEAR,
        ),
        original,
    ))
}

impl BuiltinPlugin for ImagePlugin {
    fn file_closed(&mut self, _ctx: &egui::Context, path: &Path) {
        self.loaded.remove(path);
    }
    fn id(&self) -> &'static str {
        "image"
    }
    fn name(&self) -> &'static str {
        tr!("image.image_viewer")
    }
    fn description(&self) -> &'static str {
        tr!("image.shows_images_png_jpeg_gif")
    }
    fn icon(&self) -> &'static str {
        "editorPreview"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("image")
    }

    fn opens_binary(&self, path: &Path) -> bool {
        is_image(path)
    }
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_image(path).then_some(EditorLayout::Preview)
    }
    fn layout_switch(&self, _path: &Path) -> bool {
        false
    }

    fn preview(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _text: &mut String,
        _scroll: Option<f32>,
    ) {
        let t = ui::theme(ui.ctx());
        let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
        if self.loaded.get(path).is_none_or(|l| l.modified != modified) {
            self.loaded.insert(
                path.to_path_buf(),
                Loaded {
                    modified,
                    texture: load(ui.ctx(), path),
                },
            );
        }
        let loaded = &self.loaded[path];
        let max_width = (ui.available_width() - 36.0).max(100.0);
        egui::ScrollArea::both()
            .id_salt(("image_preview", path))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| match &loaded.texture {
                        Ok((texture, [w, h])) => {
                            ui.label(
                                RichText::new(format!("{w} × {h} px"))
                                    .small()
                                    .color(t.text_secondary),
                            );
                            let scale = (max_width / *w as f32).min(1.0);
                            let size = egui::vec2(*w as f32, *h as f32) * scale;
                            ui.add(egui::Image::new((texture.id(), size)));
                        }
                        Err(e) => {
                            ui.label(RichText::new(e).color(t.error));
                        }
                    });
            });
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/image/mod/tests.rs"]
mod tests;
