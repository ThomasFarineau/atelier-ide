//! SVG built-in plugin: rendered preview next to the source or instead of it, as the Markdown
//! plugin does for `.md` files.

use crate::plugins::builtin::{BuiltinPlugin, Command, EditorLayout, Host};
use eframe::egui::{self, Margin};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct SvgPlugin {
    layouts: HashMap<PathBuf, EditorLayout>,
    /// Last image registered for each document, forgotten when the text changes.
    uris: HashMap<PathBuf, String>,
}

pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

impl BuiltinPlugin for SvgPlugin {
    fn id(&self) -> &'static str {
        "svg"
    }
    fn name(&self) -> &'static str {
        "SVG"
    }
    fn description(&self) -> &'static str {
        "Aperçu des fichiers SVG : éditeur seul, éditeur et aperçu côte à côte, ou aperçu seul."
    }
    fn icon(&self) -> &'static str {
        "editorPreview"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("svg")
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("svg.text", "SVG : éditeur seul"),
            ("svg.split", "SVG : éditeur et aperçu"),
            ("svg.preview", "SVG : aperçu seul"),
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
            "svg.text" => EditorLayout::Text,
            "svg.split" => EditorLayout::Split,
            "svg.preview" => EditorLayout::Preview,
            _ => return,
        };
        match host.active_file().filter(|p| is_svg(p)) {
            Some(path) => self.set_editor_layout(&path, layout),
            None => host.notify("Ouvrez un fichier SVG".into()),
        }
    }

    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_svg(path).then(|| self.layouts.get(path).copied().unwrap_or_default())
    }

    fn set_editor_layout(&mut self, path: &Path, layout: EditorLayout) {
        self.layouts.insert(path.to_path_buf(), layout);
    }

    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, _scroll: Option<f32>) {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let uri = format!("bytes://svg/{:x}-{}.svg", hasher.finish(), path.display());
        if let Some(old) = self.uris.insert(path.to_path_buf(), uri.clone())
            && old != uri
        {
            ui.ctx().forget_image(&old);
        }
        // The image fills the pane, whatever its own size, within the texture limit.
        let size = (ui.available_size() - egui::vec2(36.0, 36.0))
            .max(egui::vec2(100.0, 100.0))
            .min(egui::vec2(2000.0, 2000.0));
        egui::Frame::new()
            .inner_margin(Margin::same(18))
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.add(
                        egui::Image::from_bytes(uri, text.clone().into_bytes())
                            .fit_to_exact_size(size),
                    );
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_files_get_a_split_layout_by_default() {
        let mut plugin = SvgPlugin::default();
        let logo = Path::new("C:/projet/logo.SVG");
        assert_eq!(plugin.editor_layout(logo), Some(EditorLayout::Split));
        assert_eq!(plugin.editor_layout(Path::new("main.rs")), None);
        plugin.set_editor_layout(logo, EditorLayout::Preview);
        assert_eq!(plugin.editor_layout(logo), Some(EditorLayout::Preview));
    }

    #[test]
    fn preview_renders() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut plugin = SvgPlugin::default();
        let mut text = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><circle cx="10" cy="10" r="8"/></svg>"#.to_string();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plugin.preview(ui, Path::new("a.svg"), &mut text, None);
                });
            });
        }
        assert_eq!(plugin.uris.len(), 1);
    }
}
