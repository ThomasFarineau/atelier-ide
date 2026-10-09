//! Markdown built-in plugin: rendered preview next to the source or instead of it, with
//! synchronized scrolling, as IntelliJ's Markdown plugin.

use crate::{
    plugins::builtin::{BuiltinPlugin, Command, EditorLayout, Host},
    plugins::builtins::mermaid::{self, Block},
    project, ui,
};
use eframe::egui::{self, Margin};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct View {
    /// Height of the rendered document and of the visible part, from the last frame.
    content: f32,
    viewport: f32,
}

#[derive(Default)]
pub struct MarkdownPlugin {
    layouts: HashMap<PathBuf, EditorLayout>,
    views: HashMap<PathBuf, View>,
    cache: CommonMarkCache,
    /// Images of the mermaid blocks drawn by the last frame.
    diagrams: Vec<String>,
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown"))
}

/// Base URI of relative images: the folder of the document.
fn base_uri(path: &Path) -> String {
    let dir = path.parent().map(project::simplify).unwrap_or_default();
    let dir = dir.to_string_lossy().replace('\\', "/");
    let dir = dir.trim_end_matches('/');
    if dir.starts_with('/') {
        format!("file://{dir}/")
    } else {
        format!("file:///{dir}/")
    }
}

impl BuiltinPlugin for MarkdownPlugin {
    fn id(&self) -> &'static str {
        "markdown"
    }
    fn name(&self) -> &'static str {
        "Markdown"
    }
    fn description(&self) -> &'static str {
        "Aperçu des fichiers Markdown : éditeur seul, éditeur et aperçu côte à côte avec défilement synchronisé, ou aperçu seul."
    }
    fn icon(&self) -> &'static str {
        "editorPreview"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("markdown")
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("markdown.text", "Markdown : éditeur seul"),
            ("markdown.split", "Markdown : éditeur et aperçu"),
            ("markdown.preview", "Markdown : aperçu seul"),
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
            "markdown.text" => EditorLayout::Text,
            "markdown.split" => EditorLayout::Split,
            "markdown.preview" => EditorLayout::Preview,
            _ => return,
        };
        match host.active_file().filter(|p| is_markdown(p)) {
            Some(path) => self.set_editor_layout(&path, layout),
            None => host.notify("Ouvrez un fichier Markdown".into()),
        }
    }

    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        is_markdown(path).then(|| self.layouts.get(path).copied().unwrap_or_default())
    }

    fn set_editor_layout(&mut self, path: &Path, layout: EditorLayout) {
        self.layouts.insert(path.to_path_buf(), layout);
    }

    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, scroll: Option<f32>) {
        let t = ui::theme(ui.ctx());
        let view = self.views.entry(path.to_path_buf()).or_default();
        // Wide code blocks scroll inside the preview instead of widening it.
        let mut area = egui::ScrollArea::both()
            .id_salt(("markdown_preview", path))
            .auto_shrink([false, false]);
        if let Some(fraction) = scroll {
            area = area.vertical_scroll_offset(fraction * (view.content - view.viewport).max(0.0));
        }
        let width = ui.available_width();
        let output = area.show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin::symmetric(28, 18))
                .show(ui, |ui| {
                    ui.set_max_width((width - 56.0).max(200.0));
                    let style = ui.style_mut();
                    style.visuals.override_text_color = Some(t.text);
                    // Heading sizes derive from these: 28 px for a level 1 title, as in the
                    // IntelliJ preview, down to the 14 px body text.
                    style
                        .text_styles
                        .insert(egui::TextStyle::Heading, ui::semibold(28.0));
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
                    style.spacing.item_spacing.y = 8.0;
                    let max_width = (width - 80.0).max(100.0);
                    let viewer =
                        |ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &mut String| {
                            CommonMarkViewer::new()
                                .default_implicit_uri_scheme(base_uri(path))
                                .max_image_width(Some(max_width as usize))
                                .show_mut(ui, cache, text);
                        };
                    let mut blocks = mermaid::split_blocks(text);
                    if blocks.iter().all(|b| matches!(b, Block::Text(_))) {
                        viewer(ui, &mut self.cache, text);
                    } else {
                        // Mermaid blocks are drawn as diagrams between the Markdown pieces.
                        let mut edited = false;
                        let mut drawn = Vec::new();
                        for (index, block) in blocks.iter_mut().enumerate() {
                            match block {
                                Block::Text(piece) => {
                                    let before = piece.clone();
                                    ui.push_id(("markdown_piece", index), |ui| {
                                        viewer(ui, &mut self.cache, piece)
                                    });
                                    edited |= *piece != before;
                                }
                                Block::Diagram(source) => {
                                    mermaid::show(ui, source, max_width);
                                    drawn.push(mermaid::uri(source));
                                }
                            }
                        }
                        if edited {
                            // A task list item was ticked: put the pieces back together.
                            *text = blocks
                                .iter()
                                .map(|b| match b {
                                    Block::Text(piece) => piece.clone(),
                                    Block::Diagram(source) => format!(
                                        "```mermaid
{source}```
"
                                    ),
                                })
                                .collect();
                        }
                        for old in std::mem::replace(&mut self.diagrams, drawn.clone()) {
                            if !drawn.contains(&old) {
                                ui.ctx().forget_image(&old);
                            }
                        }
                    }
                });
        });
        view.content = output.content_size.y;
        view.viewport = output.inner_rect.height();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_files_get_a_split_layout_by_default() {
        let mut plugin = MarkdownPlugin::default();
        let readme = Path::new("C:/projet/README.md");
        assert_eq!(plugin.editor_layout(readme), Some(EditorLayout::Split));
        assert_eq!(plugin.editor_layout(Path::new("main.rs")), None);
        plugin.set_editor_layout(readme, EditorLayout::Preview);
        assert_eq!(plugin.editor_layout(readme), Some(EditorLayout::Preview));
        assert!(is_markdown(Path::new("Notes.MARKDOWN")));
    }

    #[test]
    fn relative_images_resolve_from_the_document_folder() {
        assert_eq!(
            base_uri(Path::new(r"\\?\C:\projet\docs\guide.md")),
            "file:///C:/projet/docs/"
        );
        assert_eq!(base_uri(Path::new("/home/a/README.md")), "file:///home/a/");
    }

    #[test]
    fn preview_renders_and_tracks_its_height() {
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut plugin = MarkdownPlugin::default();
        let path = Path::new("doc.md");
        let mut text = "# Titre\n\n- [ ] tâche\n\n```rust\nfn main() {}\n```\n".to_string();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plugin.preview(ui, path, &mut text, Some(0.5));
                });
            });
        }
        assert!(plugin.views[path].content > 0.0);
    }

    #[test]
    fn mermaid_blocks_are_drawn_as_images() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut plugin = MarkdownPlugin::default();
        let sources = [
            "graph TD\n  A[Start] --> B[Fin]\n",
            "sequenceDiagram\n  A->>B: Salut\n  B-->>A: Oui\n",
        ];
        for source in sources {
            let mut text =
                format!("# Titre\n\n```mermaid\n{source}```\n\nfin\n").replace('\n', "\r\n");
            // The diagram is drawn on a thread of its own: frames go by until it is there.
            let mut loaded = None;
            for _ in 0..600 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        plugin.preview(ui, Path::new("doc.md"), &mut text, None);
                    });
                });
                if let Some(uri) = plugin.diagrams.first() {
                    let poll = ctx.try_load_texture(uri, Default::default(), Default::default());
                    if matches!(poll, Ok(egui::load::TexturePoll::Ready { .. })) {
                        loaded = Some(poll);
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(plugin.diagrams.len(), 1, "{source}");
            assert!(loaded.is_some(), "{source}: the image never loaded");
        }
    }
}
