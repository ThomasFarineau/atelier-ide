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

mod html;

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
    fn file_closed(&mut self, _ctx: &egui::Context, path: &Path) {
        if self.views.remove(path).is_some() {
            // What the previews laid out; the ones still open lay it out again.
            self.cache = CommonMarkCache::default();
        }
    }
    fn id(&self) -> &'static str {
        "markdown"
    }
    fn name(&self) -> &'static str {
        "Markdown"
    }
    fn description(&self) -> &'static str {
        tr!("markdown.preview_markdown_files_editor_only")
    }
    fn icon(&self) -> &'static str {
        "editorPreview"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("markdown")
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("markdown.text", tr!("markdown.markdown_editor_only")),
            ("markdown.split", tr!("markdown.markdown_editor_preview")),
            ("markdown.preview", tr!("markdown.markdown_preview_only")),
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
            None => host.notify(tr!("markdown.open_markdown_file").into()),
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
                    let base = base_uri(path);
                    // egui_commonmark keeps the function of the HTML blocks as 'static.
                    let html_base = base.clone();
                    let render_html = move |ui: &mut egui::Ui, block: &str| {
                        html::show(ui, block, &html_base, max_width)
                    };
                    let show =
                        |ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &mut String| {
                            CommonMarkViewer::new()
                                .default_implicit_uri_scheme(&base)
                                .max_image_width(Some(max_width as usize))
                                .render_html_fn(Some(&render_html))
                                .show_mut(ui, cache, text);
                        };
                    let viewer =
                        |ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &mut String| {
                            // The HTML tags of the paragraphs are read as their Markdown.
                            let Some(converted) = html::inline_to_markdown(text) else {
                                show(ui, cache, text);
                                return;
                            };
                            let mut copy = converted.text.clone();
                            show(ui, cache, &mut copy);
                            // A task ticked in the converted text: the same box in the source.
                            if let Some(at) = copy
                                .bytes()
                                .zip(converted.text.bytes())
                                .position(|(a, b)| a != b)
                            {
                                let source = converted.source_offset(at);
                                text.replace_range(source..source + 1, &copy[at..at + 1]);
                            }
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
#[path = "../../../../test/plugins/builtins/markdown/mod/tests.rs"]
mod tests;
