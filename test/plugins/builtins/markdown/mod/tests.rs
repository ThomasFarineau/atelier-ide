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
    // A Windows path is only one on Windows.
    #[cfg(windows)]
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
        let mut text = format!("# Titre\n\n```mermaid\n{source}```\n\nfin\n").replace('\n', "\r\n");
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
