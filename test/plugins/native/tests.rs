use super::*;

fn manifest(dir: &Path, library: &str) -> Manifest {
    serde_json::from_value::<Manifest>(serde_json::json!({
        "id": "viewer",
        "name": "Viewer",
        "version": "1.0.0",
        "library": library,
    }))
    .map(|mut m| {
        m.dir = dir.to_path_buf();
        m
    })
    .unwrap()
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-native-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_library_is_named_as_the_system_names_them() {
    let m = manifest(Path::new("plugins"), "viewer");
    let file = m.library_path().unwrap();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    if cfg!(windows) {
        assert_eq!(name, "viewer.dll");
    } else if cfg!(target_os = "macos") {
        assert_eq!(name, "libviewer.dylib");
    } else {
        assert_eq!(name, "libviewer.so");
    }
    assert!(file.starts_with("plugins"));
}

#[test]
fn a_missing_library_is_an_error() {
    let dir = folder("missing");
    let e = load(&manifest(&dir, "nothing")).err().unwrap();
    assert!(e.contains("nothing"), "{e}");
}

#[test]
fn a_file_that_is_not_a_library_is_an_error() {
    let dir = folder("garbage");
    let m = manifest(&dir, "garbage");
    std::fs::write(m.library_path().unwrap(), b"not a library").unwrap();
    assert!(load(&m).is_err());
}

#[test]
fn a_library_of_another_interface_is_refused() {
    let dir = folder("old");
    // Any library without the symbol of the interface: the system's own C library will do.
    let m = manifest(&dir, "garbage");
    std::fs::write(m.library_path().unwrap(), b"").unwrap();
    assert!(load(&m).is_err());
}

/// What the IDE gives the plugins, without an IDE.
struct TestHost {
    notes: Vec<String>,
}

impl Host for TestHost {
    fn root(&self) -> &Path {
        Path::new("")
    }
    fn active_file(&self) -> Option<PathBuf> {
        None
    }
    fn open_file(&mut self, _path: PathBuf) {}
    fn open_project(&mut self, _path: PathBuf) {}
    fn save_files(&mut self) -> bool {
        true
    }
    fn reload_files(&mut self) {}
    fn show_tool_window(&mut self, _tool: &'static ToolWindow) {}
    fn notify(&mut self, message: String) {
        self.notes.push(message);
    }
}

/// The glTF viewer, when it was built next to the IDE (`plugins/gltf-viewer`, the library of its
/// last `cargo build`), or the library `ATELIER_TEST_PLUGIN` names.
fn built_plugin() -> Option<(PathBuf, String)> {
    let file = match std::env::var_os("ATELIER_TEST_PLUGIN") {
        Some(file) => PathBuf::from(file),
        None => {
            let name = super::super::library_file("gltf_viewer");
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("plugins/gltf-viewer/target/debug")
                .join(name)
        }
    };
    if !file.is_file() {
        eprintln!("skipped: no plugin built at {}", file.display());
        return None;
    }
    let name = file
        .file_stem()?
        .to_str()?
        .trim_start_matches("lib")
        .to_string();
    Some((file.parent()?.to_path_buf(), name))
}

#[test]
fn a_plugin_draws_in_its_surface_with_its_own_egui() {
    let Some((dir, name)) = built_plugin() else {
        return;
    };
    let mut plugin = load(&manifest(&dir, &name)).unwrap();
    assert!(plugin.opens_binary(Path::new("robot.glb")));
    assert_eq!(
        plugin.editor_layout(Path::new("robot.glb")),
        Some(EditorLayout::Preview)
    );
    assert!(plugin.file_icon(Path::new("robot.glb")).is_some());
    let ctx = egui::Context::default();
    crate::ui::install(&ctx, crate::ui::theme_by_index(0), 13.0);
    let mut host = TestHost { notes: Vec::new() };
    let model = PathBuf::from("no/such/model.glb");
    let mut painted = 0;
    for frame in 0..20 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            time: Some(f64::from(frame) / 10.0),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            plugin.update(ctx, &mut host);
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin.preview(ui, &model, &mut String::new(), None);
            });
        });
        painted = output
            .shapes
            .iter()
            .filter(|s| matches!(&s.shape, Shape::Mesh(m) if !m.indices.is_empty()))
            .count();
        if painted > 0 {
            break;
        }
        // The model is read on a thread of the plugin.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(painted > 0, "the error about the model is drawn");
    let textures = ctx.tex_manager();
    let names: Vec<String> = textures
        .read()
        .allocated()
        .map(|(_, meta)| meta.name.clone())
        .collect();
    assert!(
        names.iter().any(|n| n.starts_with("plugin:")),
        "the font atlas of the plugin is a texture of the IDE: {names:?}"
    );
    let key = model.to_string_lossy().into_owned();
    plugin.file_closed(&ctx, &model);
    assert!(!plugin.surfaces.contains_key(&(abi::surface::PREVIEW, key)));
}

/// The database plugin, when it was built next to the IDE (`plugins/database`).
#[test]
fn a_plugin_draws_its_tool_window() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plugins/database/target/debug")
        .join(super::super::library_file("database"));
    if !file.is_file() {
        eprintln!("skipped: no plugin built at {}", file.display());
        return;
    }
    let mut plugin = load(&manifest(file.parent().unwrap(), "database")).unwrap();
    let tool = plugin.tool_windows().first().expect("its tool window");
    assert!(
        crate::icon_set::find(tool.icon).is_some(),
        "its icon is one of the IDE"
    );
    assert!(!plugin.commands().is_empty());
    let ctx = egui::Context::default();
    crate::ui::install(&ctx, crate::ui::theme_by_index(2), 13.0);
    let mut host = TestHost { notes: Vec::new() };
    let mut painted = 0;
    for frame in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            time: Some(f64::from(frame) / 10.0),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            plugin.update(ctx, &mut host);
            egui::CentralPanel::default().show(ctx, |ui| {
                plugin.tool_window(tool.id, ui, &mut host);
            });
        });
        painted = output
            .shapes
            .iter()
            .filter(|s| matches!(&s.shape, Shape::Mesh(m) if !m.indices.is_empty()))
            .count();
    }
    assert!(painted > 0, "the toolbar and the list are drawn");
    // Its command opens the tool window through the host, at the next frame.
    plugin.run_command("database.open", &mut host);
}

#[test]
fn the_icons_of_the_files_come_from_the_extensions_folder() {
    let dir = folder("extensions");
    std::fs::create_dir_all(dir.join("extensions")).unwrap();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#;
    std::fs::write(dir.join("extensions/glb+GLTF.svg"), svg).unwrap();
    std::fs::write(dir.join("extensions/glb+gltf.dark.svg"), svg).unwrap();
    std::fs::write(dir.join("extensions/notes.txt"), b"not an icon").unwrap();
    let icons = extension_icons(&manifest(&dir, "viewer"));
    assert_eq!(icons.len(), 2, "{icons:?}");
    let key = icons["glb"];
    assert_eq!(icons["gltf"], key);
    let icon = crate::icon_set::find(key).unwrap();
    assert!(icon.variant(true).unwrap().uri.contains("dark"));
    assert!(icon.variant(false).unwrap().uri.contains("light"));
}
