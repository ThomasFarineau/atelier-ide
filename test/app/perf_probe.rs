use super::*;
#[test]
#[ignore]
fn probe_big_file() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let mut ide = Ide::blank(
        root,
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    );
    let ctx = egui::Context::default();
    ide.apply_theme(&ctx);
    let lines: usize = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40_000);
    let text: String = (0..lines)
        .map(|i| format!("    let value_{i} = compute(\"chaine {i}\", {i}); // commentaire\n"))
        .collect();
    let path = std::env::temp_dir().join(std::env::var("NAME").unwrap_or("atelier-big.rs".into()));
    fs::write(&path, &text).unwrap();
    println!("taille {} Ko, {} lignes", text.len() / 1024, lines);
    ide.open_file(path);
    ide.wait_loaded();
    for i in 0..4 {
        let t0 = Instant::now();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0))),
                ..Default::default()
            },
            |ctx| {
                let t = Instant::now();
                ide.status_bar(ctx);
                let a = t.elapsed();
                let t = Instant::now();
                ide.left_panel(ctx);
                let b = t.elapsed();
                let t = Instant::now();
                ide.editor(ctx);
                let c = t.elapsed();
                println!("  frame {i}: status {a:?} tree {b:?} editor {c:?}");
            },
        );
        let t = Instant::now();
        let prims = ctx.tessellate(output.shapes, output.pixels_per_point);
        let verts: usize = prims
            .iter()
            .map(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
                _ => 0,
            })
            .sum();
        println!(
            "  frame {i}: total run {:?}, tessellate {:?}, vertices {verts}",
            t0.elapsed() - t.elapsed(),
            t.elapsed()
        );
    }
}
