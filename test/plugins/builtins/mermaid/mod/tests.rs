use super::*;

const FLOW: &str = "flowchart LR\n  A[Début] --> B{Choix}\n  B -->|oui| C[Fin]\n  B -->|non| A\n";

#[test]
fn mmd_files_get_a_split_layout_by_default() {
    let mut plugin = MermaidPlugin::default();
    let file = Path::new("C:/projet/flux.MMD");
    assert_eq!(plugin.editor_layout(file), Some(EditorLayout::Split));
    assert_eq!(plugin.editor_layout(Path::new("main.rs")), None);
    plugin.set_editor_layout(file, EditorLayout::Preview);
    assert_eq!(plugin.editor_layout(file), Some(EditorLayout::Preview));
}

#[test]
fn diagrams_render_to_svg_and_export_to_png() {
    let diagram = render_blocking(FLOW).unwrap();
    assert!(String::from_utf8_lossy(&diagram.svg).contains("<svg"));
    assert!(
        diagram.size.x > 50.0 && diagram.size.x < 2000.0,
        "{:?}",
        diagram.size
    );
    let dir = std::env::temp_dir().join(format!("atelier-mmd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    export_png(&diagram, &dir.join("a.png")).unwrap();
    export_svg(&diagram, &dir.join("a.svg")).unwrap();
    assert_eq!(
        &diagram.png[1..4],
        b"PNG",
        "the picture shown is rasterized once, off the interface"
    );
    let png = std::fs::read(dir.join("a.png")).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    assert!(
        std::fs::read_to_string(dir.join("a.svg"))
            .unwrap()
            .contains("<svg")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn markdown_is_cut_around_mermaid_fences_only() {
    let doc = "# Titre\n\n```mermaid\ngraph TD\nA-->B\n```\n\n```rust\nfn main() {}\n```\n\n~~~mermaid\nA-->C\n~~~\nfin\n";
    let blocks = split_blocks(doc);
    assert_eq!(
        blocks,
        vec![
            Block::Text("# Titre\n\n".into()),
            Block::Diagram("graph TD\nA-->B\n".into()),
            Block::Text("\n```rust\nfn main() {}\n```\n\n".into()),
            Block::Diagram("A-->C\n".into()),
            Block::Text("fin\n".into()),
        ]
    );
    // Nothing lost when there is no diagram, and a mermaid fence inside another is text.
    assert_eq!(split_blocks("a\nb"), vec![Block::Text("a\nb".into())]);
    let nested = "````md\n```mermaid\nA-->B\n```\n````\n";
    assert_eq!(split_blocks(nested), vec![Block::Text(nested.into())]);
}

/// Runs frames of `plugin` until its diagram is drawn.
fn frames_until_drawn(plugin: &mut MermaidPlugin, ctx: &egui::Context, text: &mut String) {
    for _ in 0..400 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin.preview(ui, Path::new("a.mmd"), text, None);
            });
        });
        if render_async(text, ctx).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the diagram was never drawn");
}

#[test]
fn invalid_diagrams_report_an_error_instead_of_panicking() {
    let _ = render_blocking("ceci n'est pas un diagramme");
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut plugin = MermaidPlugin::default();
    let mut text = FLOW.to_string();
    frames_until_drawn(&mut plugin, &ctx, &mut text);
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            plugin.preview(ui, Path::new("a.mmd"), &mut text, None)
        });
    });
    assert_eq!(plugin.uris.len(), 1);
}

#[test]
fn a_diagram_is_drawn_in_the_background_and_shows_as_a_task() {
    let ctx = egui::Context::default();
    let source = "flowchart LR\n  X1 --> X2\n  X2 --> X3\n";
    // The first call only starts the work.
    assert!(
        render_async(source, &ctx).is_none(),
        "nothing blocks the interface"
    );
    assert!(
        crate::tasks::running()
            .iter()
            .any(|t| t.title.contains("Mermaid"))
            || render_async(source, &ctx).is_some(),
        "listed as a task while it is drawn"
    );
    let mut drawn = None;
    for _ in 0..500 {
        drawn = render_async(source, &ctx);
        if drawn.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(drawn.expect("drawn in the end").is_ok());
}

#[test]
fn a_huge_diagram_is_refused_instead_of_taking_the_window_down() {
    let huge = format!("flowchart LR\n{}", "  A --> B\n".repeat(MAX_SOURCE / 8));
    let error = render_blocking(&huge).err().expect("refused");
    assert!(error.contains("trop volumineux"), "{error}");
    assert!(export_refusal(egui::vec2(30_000.0, 20_000.0)).is_some());
    assert!(export_refusal(egui::vec2(3_000.0, 2_000.0)).is_none());
}
