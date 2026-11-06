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

/// A JSON of about 20 MB, scrolled with the wheel: the time of each frame.
#[test]
#[ignore]
fn probe_big_json_scroll() {
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
    let mut text = String::from("[\n");
    let mut i = 0;
    while text.len() < 20 * 1024 * 1024 {
        text.push_str(&format!(
            "  {{\n    \"id\": {i},\n    \"name\": \"item {i}\",\n    \"tags\": [\"a\", \"b\"],\n    \"value\": {}.5\n  }},\n",
            i * 3
        ));
        i += 1;
    }
    text.push_str("  {}\n]\n");
    // MINIFIED=1: the same JSON on one line, as an API or a build gives it.
    if std::env::var_os("MINIFIED").is_some() {
        text = text.split_whitespace().collect::<Vec<_>>().join("");
    }
    let path = std::env::temp_dir().join("atelier-big.json");
    fs::write(&path, &text).unwrap();
    println!("taille {} Mo", text.len() / 1024 / 1024);
    let t = Instant::now();
    ide.open_file(path);
    ide.wait_loaded();
    println!("ouverture {:?}", t.elapsed());
    let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0));
    let mut slowest = std::time::Duration::ZERO;
    for frame in 0..40 {
        let mut events = vec![egui::Event::PointerMoved(egui::pos2(800.0, 450.0))];
        if frame > 2 {
            events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -4000.0),
                modifiers: Default::default(),
            });
        }
        let t0 = Instant::now();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                let t = Instant::now();
                ide.status_bar(ctx);
                let a = t.elapsed();
                ide.left_panel(ctx);
                let t = Instant::now();
                ide.editor(ctx);
                let c = t.elapsed();
                print!("  status {a:?} editor {c:?} |");
            },
        );
        let run = t0.elapsed();
        let t = Instant::now();
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        let tess = t.elapsed();
        slowest = slowest.max(run + tess);
        println!("  frame {frame}: run {run:?}, tessellate {tess:?}");
    }
    println!("plus lente {slowest:?}");
}

/// The ^C a terminal sends, without then with the Ctrl+C of the process allowed again: does it
/// stop `ping -t`? Run it from a process group that ignores Ctrl+C to see the difference, e.g.
/// `Start-Process -NoNewWindow` is not enough; the launcher's CREATE_NEW_PROCESS_GROUP is.
#[test]
#[ignore]
fn probe_terminal_interrupt() {
    use crate::terminal::{Shell, Terminal};
    let shell = Shell {
        name: "PowerShell".into(),
        program: "powershell.exe".into(),
        args: vec!["-NoLogo".into(), "-NoProfile".into(), "-NoExit".into()],
    };
    let wait = |term: &mut Terminal, ms: u64| {
        let until = Instant::now() + std::time::Duration::from_millis(ms);
        while Instant::now() < until {
            term.poll();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    };
    let run = |label: &str| {
        let mut term = Terminal::start(&std::env::temp_dir(), &shell, &[]).unwrap();
        term.resize(30, 120);
        wait(&mut term, 3000);
        term.command("ping -t 127.0.0.1").unwrap();
        wait(&mut term, 2500);
        term.write(&[3]).unwrap();
        wait(&mut term, 1500);
        term.command("echo after-interrupt").unwrap();
        wait(&mut term, 1500);
        let text = term.screen_ref().contents();
        let stopped = text.lines().any(|l| l.trim() == "after-interrupt");
        println!("{label}: interrompu = {stopped}");
    };
    let ignored = unsafe {
        // What the launcher does to the IDE: Ctrl+C ignored, inherited by the children.
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 1)
    };
    println!("Ctrl+C ignoré comme sous le lanceur ({ignored})");
    run("sans le correctif");
    crate::allow_ctrl_c();
    run("avec le correctif");
}

/// Switching between the tabs of real files: the time of the frames right after each switch.
#[test]
#[ignore]
fn probe_tab_switch() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let mut ide = Ide::blank(
        root.clone(),
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    );
    let ctx = egui::Context::default();
    ide.apply_theme(&ctx);
    let files = [
        "src/app.rs",
        "src/editor.rs",
        "README.md",
        "Cargo.toml",
        "src/plugins/builtins/node/package.schema.json",
        "src/languages/json/mod.rs",
    ];
    for f in files {
        ide.open_file(root.join(f));
    }
    ide.wait_loaded();
    let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0));
    let frame = |ide: &mut Ide| {
        let t0 = Instant::now();
        let mut parts = (Default::default(), Default::default(), Default::default());
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                let t = Instant::now();
                ide.status_bar(ctx);
                parts.0 = t.elapsed();
                let t = Instant::now();
                ide.left_panel(ctx);
                parts.1 = t.elapsed();
                let t = Instant::now();
                ide.editor(ctx);
                parts.2 = t.elapsed();
            },
        );
        let t = Instant::now();
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        (t0.elapsed(), parts, t.elapsed())
    };
    // Every tab seen once, then switched to again: the second round is what a user feels.
    for round in 0..2 {
        for i in 0..files.len() {
            ide.active = i;
            let mut line = format!("tour {round} {:<45}", files[i]);
            for n in 0..4 {
                let (total, (s, l, e), tess) = frame(&mut ide);
                if n == 0 {
                    line += &format!(
                        " 1re image {total:?} (statut {s:?}, arbre {l:?}, éditeur {e:?}, tess {tess:?})"
                    );
                } else {
                    line += &format!(" | {total:?}");
                }
            }
            println!("{line}");
            // Background work (language servers, git) gets a moment, as between two clicks.
            std::thread::sleep(std::time::Duration::from_millis(300));
            frame(&mut ide);
        }
    }
}
