use super::*;

fn ide() -> Ide {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    Ide::blank(
        root,
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    )
}

/// Ctrl+Alt+L reformats the active document, here a JSON file the IDE formats itself, and
/// says what it did.
#[test]
fn reformat_code_formats_the_active_document() {
    let dir = std::env::temp_dir().join(format!("atelier-reformat-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.json");
    fs::write(&path, "{\"b\":1,\"a\":[true]}\n").unwrap();
    let ctx = egui::Context::default();
    let mut ide = ide();
    ide.apply_theme(&ctx);
    ide.open_file(path);
    ide.wait_loaded();
    frame(&ctx, &mut ide);
    ide.reformat_code();
    for _ in 0..4 {
        frame(&ctx, &mut ide);
    }
    assert_eq!(
        ide.documents[ide.active].text,
        "{\n  \"b\": 1,\n  \"a\": [\n    true\n  ]\n}\n"
    );
    assert_eq!(ide.status, "6 ligne(s) reformatée(s)");
    ide.reformat_code();
    for _ in 0..3 {
        frame(&ctx, &mut ide);
    }
    assert!(
        ide.status.starts_with("Aucune ligne modifiée"),
        "{}",
        ide.status
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The settings dialog edits a draft: Apply makes it the settings, a new opening forgets it.
#[test]
fn settings_change_only_when_applied() {
    let ctx = egui::Context::default();
    let mut ide = ide();
    let theme = ide.settings.theme;
    let other = (theme + 1) % ui::THEMES.len();
    ide.open_settings(Page::Appearance);
    ide.settings_draft.theme = other;
    assert_eq!(ide.settings.theme, theme, "still a draft");
    ide.apply_settings(&ctx);
    assert_eq!(ide.settings.theme, other);
    ide.open_settings(Page::Plugins);
    ide.settings_draft.disabled_builtins.insert("git".into());
    // Cancel: the dialog closes without applying.
    ide.show_settings = false;
    assert!(ide.builtin_enabled("git"));
    ide.open_settings(Page::Plugins);
    assert!(
        !ide.settings_draft.disabled_builtins.contains("git"),
        "a new draft"
    );
    ide.settings_draft.disabled_builtins.insert("git".into());
    ide.apply_settings(&ctx);
    assert!(!ide.builtin_enabled("git"));
}

fn frame_with(ctx: &egui::Context, ide: &mut Ide, time: f64, events: Vec<egui::Event>) {
    frame_mod(ctx, ide, time, events, Default::default());
}

fn frame_mod(
    ctx: &egui::Context,
    ide: &mut Ide,
    time: f64,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) {
    let _ = ctx.run(
        egui::RawInput {
            modifiers,
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0))),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| {
            ide.track_focus(ctx);
            ide.left_panel(ctx);
            ide.editor(ctx);
            ide.dialogs(ctx);
        },
    );
}

#[test]
fn ctrl_and_shift_select_several_entries_in_the_tree() {
    use egui::{Event, Modifiers, PointerButton};
    let base = std::env::temp_dir().join(format!("atelier-multi-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("dest")).unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        fs::write(base.join(name), "x").unwrap();
    }
    let base = base.canonicalize().unwrap();
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut ide = Ide::blank(
        base.clone(),
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    );
    ide.show_project = true;
    let mut time = 0.0;
    let mut step = |ide: &mut Ide, events: Vec<Event>, modifiers: Modifiers| {
        time += 0.05;
        frame_mod(&ctx, ide, time, events, modifiers);
    };
    step(&mut ide, vec![], Modifiers::NONE);
    step(&mut ide, vec![], Modifiers::NONE);
    let click = |ide: &mut Ide,
                 name: &str,
                 modifiers: Modifiers,
                 step: &mut dyn FnMut(&mut Ide, Vec<Event>, Modifiers)| {
        let at = ide
            .row_rects
            .iter()
            .rev()
            .find(|(p, _)| p.ends_with(name))
            .unwrap()
            .1
            .center();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        };
        step(ide, vec![Event::PointerMoved(at)], modifiers);
        step(ide, vec![button(true)], modifiers);
        step(ide, vec![button(false)], modifiers);
        step(ide, vec![], modifiers);
    };
    click(&mut ide, "a.txt", Modifiers::NONE, &mut step);
    click(&mut ide, "c.txt", Modifiers::CTRL, &mut step);
    click(&mut ide, "d.txt", Modifiers::CTRL, &mut step);
    let want: BTreeSet<_> = ["a.txt", "c.txt", "d.txt"]
        .iter()
        .map(|n| base.join(n))
        .collect();
    assert_eq!(ide.tree_multi, want);
    // Ctrl on a selected row takes it out of the selection.
    click(&mut ide, "c.txt", Modifiers::CTRL, &mut step);
    assert!(!ide.tree_multi.contains(&base.join("c.txt")) && ide.tree_multi.len() == 2);
    // Shift selects the rows between the anchor and the click.
    click(&mut ide, "b.txt", Modifiers::NONE, &mut step);
    assert!(ide.tree_multi.is_empty());
    click(&mut ide, "d.txt", Modifiers::SHIFT, &mut step);
    let want: BTreeSet<_> = ["b.txt", "c.txt", "d.txt"]
        .iter()
        .map(|n| base.join(n))
        .collect();
    assert_eq!(ide.tree_multi, want);
    assert_eq!(ide.tree_selection().len(), 3);
    // Several documents are not opened by these clicks.
    assert!(
        ide.documents
            .iter()
            .all(|d| d.path.as_deref() != Some(&base.join("c.txt")))
    );
    // Dragging one of them onto the folder moves them all.
    let from = ide
        .row_rects
        .iter()
        .rev()
        .find(|(p, _)| p.ends_with("c.txt"))
        .unwrap()
        .1
        .center();
    let to = ide
        .row_rects
        .iter()
        .rev()
        .find(|(p, _)| p.ends_with("dest"))
        .unwrap()
        .1
        .center();
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    step(&mut ide, vec![Event::PointerMoved(from)], Modifiers::NONE);
    step(&mut ide, vec![button(from, true)], Modifiers::NONE);
    for i in 1..=6 {
        let pos = from + (to - from) * (i as f32 / 6.0);
        step(&mut ide, vec![Event::PointerMoved(pos)], Modifiers::NONE);
    }
    step(&mut ide, vec![button(to, false)], Modifiers::NONE);
    step(&mut ide, vec![], Modifiers::NONE);
    for name in ["b.txt", "c.txt", "d.txt"] {
        assert!(base.join("dest").join(name).exists(), "{name} moved");
        assert!(!base.join(name).exists());
    }
    assert!(base.join("a.txt").exists());
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn the_tree_follows_changes_made_outside() {
    let base = std::env::temp_dir().join(format!("atelier-refresh-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("sub")).unwrap();
    fs::write(base.join("a.txt"), "x").unwrap();
    let base = base.canonicalize().unwrap();
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut ide = Ide::blank(
        base.clone(),
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    );
    ide.show_project = true;
    ide.expanded.insert(base.join("sub"));
    let mut time = 0.0;
    let mut step = |ide: &mut Ide| {
        time += 0.05;
        frame_with(&ctx, ide, time, vec![]);
    };
    step(&mut ide);
    step(&mut ide);
    let names = |ide: &Ide, dir: &Path| -> Vec<String> {
        ide.tree[dir]
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(names(&ide, &base), ["sub", "a.txt"]);
    std::thread::sleep(Duration::from_millis(30));
    fs::write(base.join("new.txt"), "x").unwrap();
    fs::write(base.join("sub/inner.txt"), "x").unwrap();
    fs::remove_file(base.join("a.txt")).unwrap();
    ide.last_watch = Instant::now() - Duration::from_secs(2);
    ide.watch_files(&ctx);
    step(&mut ide);
    step(&mut ide);
    assert_eq!(names(&ide, &base), ["sub", "new.txt"]);
    assert_eq!(names(&ide, &base.join("sub")), ["inner.txt"]);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn arrow_keys_move_the_selection_in_the_tree() {
    use egui::{Event, PointerButton};
    let base = std::env::temp_dir().join(format!("atelier-keys-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("dest")).unwrap();
    fs::write(base.join("a.txt"), "x").unwrap();
    fs::write(base.join("b.txt"), "x").unwrap();
    let base = base.canonicalize().unwrap();
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut ide = Ide::blank(
        base.clone(),
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    );
    ide.show_project = true;
    let mut time = 0.0;
    let mut step = |ide: &mut Ide, events: Vec<Event>| {
        time += 0.05;
        frame_with(&ctx, ide, time, events);
    };
    step(&mut ide, vec![]);
    step(&mut ide, vec![]);
    let at = ide
        .row_rects
        .iter()
        .rev()
        .find(|(p, _)| p.ends_with("dest"))
        .unwrap()
        .1
        .center();
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    step(&mut ide, vec![Event::PointerMoved(at)]);
    step(&mut ide, vec![button(true)]);
    step(&mut ide, vec![button(false)]);
    step(&mut ide, vec![]);
    assert_eq!(ide.tree_selected, Some(base.join("dest")));
    let key = |key| Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    step(&mut ide, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(
        ide.tree_selected,
        Some(base.join("a.txt")),
        "focus: {:?}",
        ide.focus
    );
    step(&mut ide, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(ide.tree_selected, Some(base.join("b.txt")));
    step(&mut ide, vec![key(egui::Key::ArrowUp)]);
    assert_eq!(ide.tree_selected, Some(base.join("a.txt")));
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn dragging_a_file_onto_a_folder_moves_it() {
    use egui::{Event, PointerButton};
    let base = std::env::temp_dir().join(format!("atelier-dnd-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("dest")).unwrap();
    fs::write(base.join("a.txt"), "x").unwrap();
    let base = base.canonicalize().unwrap();
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let mut ide = Ide::blank(
        base.clone(),
        Settings {
            font_size: 14.0,
            ..Default::default()
        },
        None,
    );
    ide.show_project = true;
    let mut time = 0.0;
    let mut step = |ide: &mut Ide, events: Vec<Event>| {
        time += 0.05;
        frame_with(&ctx, ide, time, events);
    };
    step(&mut ide, vec![]);
    step(&mut ide, vec![]);
    let center = |ide: &Ide, name: &str| {
        ide.row_rects
            .iter()
            .rev()
            .find(|(p, _)| p.file_name().is_some_and(|n| n == name))
            .unwrap()
            .1
            .center()
    };
    let (from, to) = (center(&ide, "a.txt"), center(&ide, "dest"));
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    step(&mut ide, vec![Event::PointerMoved(from)]);
    step(&mut ide, vec![button(from, true)]);
    for i in 1..=8 {
        let at = from + (to - from) * (i as f32 / 8.0);
        step(&mut ide, vec![Event::PointerMoved(at)]);
    }
    assert!(ide.tree_drag.is_some(), "the drag started");
    assert_eq!(
        ide.tree_drop.as_deref(),
        Some(base.join("dest").as_path()),
        "the folder is the target"
    );
    step(&mut ide, vec![button(to, false)]);
    step(&mut ide, vec![]);
    assert!(base.join("dest/a.txt").exists(), "{}", ide.status);
    assert!(!base.join("a.txt").exists());
    fs::remove_dir_all(&base).unwrap();
}

fn frame(ctx: &egui::Context, ide: &mut Ide) {
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0))),
            ..Default::default()
        },
        |ctx| {
            ide.each_builtin(|plugin, ide| plugin.update(ctx, ide));
            ide.toolbar(ctx);
            ide.status_bar(ctx);
            ide.stripe(ctx, Dock::Left);
            ide.stripe(ctx, Dock::Right);
            ide.left_panel(ctx);
            ide.right_panel(ctx);
            ide.bottom_panel(ctx);
            ide.editor(ctx);
            ide.dialogs(ctx);
            ide.log_status();
            ide.balloons(ctx);
        },
    );
    assert!(
        !ctx.tessellate(output.shapes, output.pixels_per_point)
            .is_empty()
    );
}

#[test]
fn dirty_documents_block_close_project_switch_and_exit() {
    let mut ide = ide();
    let mut doc = Document::untitled();
    doc.text = "unsaved".into();
    ide.documents.push(doc);
    ide.request(Pending::Close(0));
    assert!(matches!(ide.pending, Some(Pending::Close(0))));
    assert_eq!(ide.documents.len(), 1);
    let original = ide.root.clone();
    ide.request(Pending::Project(original.join("examples/demo")));
    assert!(matches!(ide.pending, Some(Pending::Project(_))));
    assert_eq!(ide.root, original);
    ide.request(Pending::Quit);
    assert!(matches!(ide.pending, Some(Pending::Quit)));
    assert!(!ide.can_quit);
}

#[test]
fn opening_another_project_asks_for_the_window() {
    let mut ide = ide();
    let demo = ide.root.join("examples/demo");
    ide.open_project(demo.clone());
    assert_eq!(ide.project_choice, Some(demo.canonicalize().unwrap()));
    let original = ide.root.clone();
    ide.open_project(original);
    assert_eq!(
        ide.project_choice.as_ref().map(|p| p.ends_with("demo")),
        Some(true)
    );
}

#[test]
fn dev_reload_keeps_project_tabs_and_unsaved_text() {
    let mut ide = ide();
    ide.open_file(ide.root.join("examples/demo/hello.py"));
    ide.open_file(ide.root.join("examples/demo/app.js"));
    ide.wait_loaded();
    ide.documents[0].text.push_str("# pas encore enregistré\n");
    ide.documents.push(Document::untitled());
    ide.documents[2].text = "brouillon".into();
    ide.active = 1;
    ide.show_terminal = true;
    let path = std::env::temp_dir().join(format!("atelier-session-{}.json", std::process::id()));
    ide.save_session(&path).unwrap();

    let mut restored = super::tests::ide();
    restored.root = std::env::temp_dir();
    restored.restore_session(&path);
    assert_eq!(restored.root, ide.root);
    assert_eq!(restored.documents.len(), 3);
    assert!(restored.documents[0].dirty());
    assert!(
        restored.documents[0]
            .text
            .ends_with("# pas encore enregistré\n")
    );
    assert!(!restored.documents[1].dirty());
    assert_eq!(restored.documents[2].text, "brouillon");
    assert_eq!(restored.active, 1);
    assert!(restored.show_terminal);
    assert!(!path.exists(), "the session file is consumed on restore");

    // Built-in tool windows survive the reload too.
    let commit = restored.tool_windows()[0];
    restored.open_tool(commit);
    restored.save_session(&path).unwrap();
    let mut again = super::tests::ide();
    again.restore_session(&path);
    assert!(again.tool_visible(commit));
}

#[test]
fn dropped_tabs_split_the_pane_on_the_side_they_land() {
    let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
    assert!(!drop_zone(r, pos2(10.0, 50.0)).0);
    assert!(drop_zone(r, pos2(10.0, 50.0)).1, "left of the pane");
    assert!(!drop_zone(r, pos2(90.0, 50.0)).1, "right of the pane");
    let (stacked, before, zone) = drop_zone(r, pos2(50.0, 20.0));
    assert!(stacked && before, "top half");
    assert_eq!(zone.height(), 50.0);
    assert!(!drop_zone(r, pos2(50.0, 80.0)).1, "bottom half");
}

#[test]
fn opening_the_same_file_reuses_its_tab() {
    let mut ide = ide();
    let path = ide.root.join("examples/demo/src/main.rs");
    ide.open_file(path.clone());
    ide.open_file(path);
    ide.wait_loaded();
    assert_eq!(ide.documents.len(), 1);
    assert_eq!(ide.documents[0].language, "rs");
    ide.request(Pending::Close(0));
    assert!(ide.documents.is_empty());
    assert_eq!(ide.active, 0);
}

#[test]
fn errors_become_balloons_and_notes() {
    let mut ide = ide();
    ide.status = "Erreur : disque plein".into();
    ide.log_status();
    assert_eq!(ide.notes.len(), 1);
    assert!(ide.notes[0].error);
    assert_eq!(ide.balloons, [0]);
    ide.status = tr!("saved").into();
    ide.log_status();
    assert_eq!(ide.notes.len(), 2);
    assert_eq!(
        ide.balloons,
        [0],
        "plain status messages only go to the log"
    );
}

#[test]
fn search_everywhere_finds_project_files() {
    let mut ide = ide();
    ide.open_palette();
    ide.palette_query = "cargo.tom".into();
    // The files are listed in the background: the popup does not wait for them.
    let started = std::time::Instant::now();
    let mut hits = ide.palette_hits();
    while hits.is_empty() && started.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        hits = ide.palette_hits();
    }
    assert!(
        hits.iter()
            .any(|h| matches!(&h.target, Target::File(p) if p.ends_with("Cargo.toml")))
    );
}

#[test]
fn initials_follow_intellij() {
    assert_eq!(initials("integration-rfe"), "IR");
    assert_eq!(initials("rust"), "RU");
}

#[test]
fn all_languages_and_themes_render_without_panicking() {
    let mut ide = ide();
    let ctx = egui::Context::default();
    for file in [
        "index.html",
        "style.css",
        "app.js",
        "hello.ts",
        "Main.java",
        "hello.py",
        "src/main.rs",
    ] {
        ide.open_file(ide.root.join("examples/demo").join(file));
        ide.wait_loaded();
    }
    assert_eq!(ide.documents.len(), 7);
    ide.show_terminal = false;
    ide.show_notifications = true;
    for theme in 0..ui::THEMES.len() {
        ide.settings.theme = theme;
        ide.apply_theme(&ctx);
        for active in 0..7 {
            ide.active = active;
            frame(&ctx, &mut ide);
        }
    }
    for (page, _) in Page::TREE {
        ide.open_settings(page);
        frame(&ctx, &mut ide);
    }
    ide.show_settings = false;
    ide.open_palette();
    ide.palette_query = "main".into();
    frame(&ctx, &mut ide);
}

/// Drives the frames of a real session on this repository with the Git tool windows open.
#[test]
fn git_plugin_windows_render_on_this_repository() {
    let mut ide = ide();
    ide.open_file(ide.root.join("src/main.rs"));
    ide.wait_loaded();
    let ctx = egui::Context::default();
    ide.apply_theme(&ctx);
    let tools = ide.tool_windows();
    assert_eq!(
        tools.len(),
        7,
        "Commit, Git, Kubernetes, Search, Problems, AI sessions and Runtimes: no npm, Maven or Gradle here"
    );
    for tool in tools {
        ide.open_tool(tool);
    }
    frame(&ctx, &mut ide);
    ide.run_builtin_command("git", "git.annotate");
    // Status, log and blame load in the background.
    for _ in 0..100 {
        frame(&ctx, &mut ide);
        if ide.gutter().annotations.is_some_and(|a| a != "Chargement…") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let annotations = ide.gutter().annotations.unwrap();
    assert_eq!(
        annotations.lines().count(),
        ide.documents[0].text.lines().count(),
        "one blame annotation per line"
    );
    for command in [
        "git.file_diff",
        "git.push",
        "git.branch",
        "git.stash",
        "git.remotes",
    ] {
        ide.run_builtin_command("git", command);
        frame(&ctx, &mut ide);
    }
}

/// The settings go to a file of their own, and come back from it.
#[test]
fn settings_are_saved_in_a_file_and_read_back() {
    let dir = std::env::temp_dir().join(format!("atelier-settings-{}", std::process::id()));
    let path = dir.join("settings.json");
    let _ = fs::remove_dir_all(&dir);
    assert!(load_settings(&path).is_none(), "nothing yet");
    let settings = Settings {
        theme: 2,
        font_size: 16.0,
        language: Some("en_US".into()),
        recent: vec![PathBuf::from("/work/shop")],
        ..Default::default()
    };
    save_settings(&path, &settings).unwrap();
    let back = load_settings(&path).unwrap();
    assert_eq!(back.theme, 2);
    assert_eq!(back.font_size, 16.0);
    assert_eq!(back.language.as_deref(), Some("en_US"));
    assert_eq!(back.recent, [PathBuf::from("/work/shop")]);
    assert!(!dir.join("settings.json.tmp").exists());
    let _ = fs::remove_dir_all(&dir);
}

/// A Ctrl + click that opens another file leaves the button down over the new editor: moving
/// the mouse before releasing it must not select in that file.
#[test]
fn a_press_begun_in_another_file_does_not_select() {
    use egui::{Event, PointerButton, pos2};
    let ctx = egui::Context::default();
    let mut ide = ide();
    ide.apply_theme(&ctx);
    ide.open_file(ide.root.join("examples/demo/hello.py"));
    ide.open_file(ide.root.join("examples/demo/app.js"));
    ide.wait_loaded();
    ide.active = 0;
    let mut time = 0.0;
    let mut step = |ide: &mut Ide, events: Vec<Event>| {
        time += 0.05;
        frame_with(&ctx, ide, time, events);
    };
    step(&mut ide, vec![]);
    step(&mut ide, vec![]);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let from = pos2(400.0, 100.0);
    step(&mut ide, vec![Event::PointerMoved(from)]);
    step(&mut ide, vec![button(from, true)]);
    // The jump to the declaration opens the other file while the button is still down.
    ide.active = 1;
    step(&mut ide, vec![]);
    let to = pos2(700.0, 300.0);
    for i in 1..=6 {
        let pos = from + (to - from) * (i as f32 / 6.0);
        step(&mut ide, vec![Event::PointerMoved(pos)]);
    }
    step(&mut ide, vec![button(to, false)]);
    assert!(ide.documents[1].view.selection().is_empty());
}

/// Shift+F6 opens the rename field on the name at the caret, and Enter renames it everywhere:
/// the single-line field gives up the focus in the frame where Enter is pressed.
#[test]
fn shift_f6_then_enter_renames_the_name_at_the_caret() {
    use egui::{Event, Key, Modifiers, PointerButton, pos2};
    let ctx = egui::Context::default();
    let mut ide = ide();
    ide.apply_theme(&ctx);
    ide.open_file(ide.root.join("examples/demo/app.js"));
    ide.wait_loaded();
    let mut time = 0.0;
    let mut step = |ide: &mut Ide, events: Vec<Event>, modifiers: Modifiers| {
        time += 0.05;
        frame_mod(&ctx, ide, time, events, modifiers);
    };
    step(&mut ide, vec![], Modifiers::NONE);
    step(&mut ide, vec![], Modifiers::NONE);
    // A click gives the editor the keyboard.
    let at = pos2(400.0, 100.0);
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    step(
        &mut ide,
        vec![Event::PointerMoved(at), button(true)],
        Modifiers::NONE,
    );
    step(&mut ide, vec![button(false)], Modifiers::NONE);
    let caret = ide.documents[0].text.find("message").unwrap() + 1;
    ide.documents[0].view.cursor = caret;
    ide.documents[0].view.anchor = caret;
    let key = |key, modifiers| Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    };
    step(
        &mut ide,
        vec![key(Key::F6, Modifiers::SHIFT)],
        Modifiers::SHIFT,
    );
    step(&mut ide, vec![], Modifiers::NONE);
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new("rename_field"))
    );
    step(
        &mut ide,
        vec![Event::Text("greeting".into())],
        Modifiers::NONE,
    );
    step(
        &mut ide,
        vec![key(Key::Enter, Modifiers::NONE)],
        Modifiers::NONE,
    );
    for _ in 0..40 {
        step(&mut ide, vec![], Modifiers::NONE);
        if !ide.documents[0].text.contains("message") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = &ide.documents[0].text;
    assert!(!text.contains("message"), "{text}");
    assert_eq!(text.matches("greeting").count(), 3);
}

/// Dragging the mouse selects from the character the button went down on, not from where the
/// drag was noticed a few pixels further.
#[test]
fn a_mouse_drag_selects_from_where_the_button_went_down() {
    use egui::{Event, Modifiers, PointerButton, pos2};
    let ctx = egui::Context::default();
    let mut ide = ide();
    ide.apply_theme(&ctx);
    ide.open_file(ide.root.join("examples/demo/app.js"));
    ide.wait_loaded();
    let mut time = 0.0;
    let mut step = |ide: &mut Ide, events: Vec<Event>| {
        time += 0.05;
        frame_with(&ctx, ide, time, events);
    };
    step(&mut ide, vec![]);
    step(&mut ide, vec![]);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let from = pos2(400.0, 100.0);
    step(
        &mut ide,
        vec![Event::PointerMoved(from), button(from, true)],
    );
    let pressed_at = ide.documents[0].view.cursor;
    let to = pos2(600.0, 100.0);
    for i in 1..=8 {
        step(
            &mut ide,
            vec![Event::PointerMoved(from + (to - from) * (i as f32 / 8.0))],
        );
    }
    step(&mut ide, vec![button(to, false)]);
    let view = &ide.documents[0].view;
    assert_eq!(view.anchor, pressed_at);
    assert!(view.cursor > pressed_at, "{} {}", view.anchor, view.cursor);
}

/// Frames of the editor with the second editor, the sticky lines, the breadcrumbs, the hints and
/// several carets all on: nothing panics, and the second editor goes away with its file.
#[test]
fn the_editor_draws_with_a_split_and_several_carets() {
    let dir = std::env::temp_dir().join(format!("atelier-split-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let first = dir.join("a.rs");
    let second = dir.join("b.rs");
    let code: String = (0..80)
        .map(|i| format!("fn f{i}() {{\n    let x = {i};\n    let y = x;\n}}\n"))
        .collect();
    fs::write(&first, &code).unwrap();
    fs::write(&second, "fn other() {}\n").unwrap();
    let mut ide = ide();
    ide.open_file(first.clone());
    ide.wait_loaded();
    ide.open_file(second.clone());
    ide.wait_loaded();
    let ctx = egui::Context::default();
    ide.apply_theme(&ctx);
    let frame = |ide: &mut Ide| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))),
                ..Default::default()
            },
            |ctx| ide.editor(ctx),
        );
    };
    // The same file twice.
    ide.active = 0;
    ide.split_right();
    assert!(ide.split.is_some());
    frame(&mut ide);
    frame(&mut ide);
    // The second editor on another file.
    ide.active = 1;
    frame(&mut ide);
    assert_eq!(ide.split_other(), Some(0));
    ide.documents[1].view.select(0, 0);
    frame(&mut ide);
    // Closing the file closes the split.
    ide.documents.remove(0);
    ide.active = 0;
    frame(&mut ide);
    assert!(ide.split.is_none());
    let _ = fs::remove_dir_all(dir);
}

/// A plugin that ignores the files named `*.tmp`, as another ignore file would.
struct IgnoresTmp;

impl BuiltinPlugin for IgnoresTmp {
    fn id(&self) -> &'static str {
        "ignores-tmp"
    }
    fn name(&self) -> &'static str {
        "Ignores tmp"
    }
    fn description(&self) -> &'static str {
        ""
    }
    fn ignored_by(&self, path: &Path) -> Option<String> {
        (path.extension().is_some_and(|e| e == "tmp")).then(|| ".tmpignore".to_string())
    }
}

/// Any plugin can say what an ignore file ignores; the tree shows it, or leaves it out when the
/// eye is off.
#[test]
fn plugins_say_what_is_ignored_and_the_tree_can_leave_it_out() {
    let dir = std::env::temp_dir().join(format!("atelier-ignored-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("keep.rs"), "").unwrap();
    fs::write(dir.join("scratch.tmp"), "").unwrap();
    let mut ide = Ide::blank(dir.clone(), Settings { font_size: 14.0, ..Default::default() }, None);
    ide.builtins.push(Box::new(IgnoresTmp));
    let ctx = egui::Context::default();
    ide.apply_theme(&ctx);
    let shown = |ide: &mut Ide| -> Vec<String> {
        ide.tree.clear();
        ide.row_rects.clear();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 600.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let root = ide.root.clone();
                    ide.tree_ui(ui, &root, 0);
                });
            },
        );
        ide.row_rects
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(ide.ignored_by(&dir.join("scratch.tmp")).as_deref(), Some(".tmpignore"));
    assert_eq!(ide.ignored_by(&dir.join("keep.rs")), None);
    assert_eq!(shown(&mut ide), ["keep.rs", "scratch.tmp"]);
    ide.settings.hide_ignored = true;
    assert_eq!(shown(&mut ide), ["keep.rs"]);
    let _ = fs::remove_dir_all(dir);
}

/// Several windows share the settings file: a window writes what it changed, and keeps what
/// the others wrote (a font chosen in one window is not put back by another).
#[test]
fn a_window_does_not_put_back_what_another_changed() {
    let base = serde_json::json!({"theme": 0, "fonts": {"code": "A"}, "recent": ["/a"]});
    // Another window chose a font and wrote it.
    let mut file = serde_json::json!({"theme": 0, "fonts": {"code": "B"}, "recent": ["/a"]});
    // This window only opened another project.
    let mine = serde_json::json!({"theme": 0, "fonts": {"code": "A"}, "recent": ["/b", "/a"]});
    merge_settings(&mut file, &mine, &base);
    assert_eq!(file["fonts"]["code"], "B", "the font of the other window stays");
    assert_eq!(file["recent"], serde_json::json!(["/b", "/a"]), "what this window changed is written");
}
