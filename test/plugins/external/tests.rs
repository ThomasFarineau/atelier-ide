use super::*;
#[test]
fn arguments_are_quoted_as_literals() {
    let cmd = shell_command(
        "python",
        &["a b.py".into(), "$(echo injected)".into(), "it's.py".into()],
    );
    assert!(cmd.contains("'a b.py'"));
    assert!(cmd.contains("'$(echo injected)'"));
    if cfg!(windows) {
        assert!(cmd.contains("'it''s.py'"));
    } else {
        assert!(cmd.contains("'it'\\''s.py'"));
    }
}

#[test]
fn manifests_are_files_or_folders_and_name_their_library_only() {
    let root = std::env::temp_dir().join(format!("atelier-manifests-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let plugins = root.join(".atelier/plugins");
    fs::create_dir_all(plugins.join("viewer")).unwrap();
    fs::create_dir_all(plugins.join("empty")).unwrap();
    fs::write(
        plugins.join("tools.json"),
        r#"{ "id": "tools", "name": "Tools", "version": "1" }"#,
    )
    .unwrap();
    fs::write(
        plugins.join("viewer/plugin.json"),
        r#"{ "id": "viewer", "name": "Viewer", "version": "1", "library": "viewer" }"#,
    )
    .unwrap();
    fs::write(
        plugins.join("escape.json"),
        r#"{ "id": "escape", "name": "Escape", "version": "1", "library": "../../evil" }"#,
    )
    .unwrap();
    let (loaded, errors) = load_with(&root, &root.join("installed"));
    let ids: Vec<_> = loaded.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["tools", "viewer"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("escape.json"));
    assert!(loaded[0].library_path().is_none());
    assert_eq!(
        loaded[1].library_path().unwrap().parent().unwrap(),
        plugins.join("viewer")
    );
    let _ = fs::remove_dir_all(&root);
}

/// The file of a plugin: a zip of `files`.
fn package(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    use std::io::Write;
    let path = dir.join("plugin.atelier-plugin");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for (name, data) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-install-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_plugin_installs_from_its_file_for_every_project_and_uninstalls() {
    let dir = temp("roundtrip");
    let (installed, project) = (dir.join("installed"), dir.join("project"));
    fs::create_dir_all(&project).unwrap();
    let library = format!(
        "{}/{}viewer{}",
        platform(),
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let file = package(
        &dir,
        &[
            (
                "plugin.json",
                br#"{ "id": "viewer", "name": "Viewer", "version": "1.0.0", "library": "viewer", "abi": 2 }"#,
            ),
            (&library, b"library"),
        ],
    );
    assert_eq!(
        install_into(&file, &installed),
        Ok(Installed::Now("Viewer".into()))
    );
    let (plugins, errors) = load_with(&project, &installed);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(plugins.len(), 1);
    assert!(plugins[0].installed);
    assert_eq!(plugins[0].abi, Some(2));
    assert_eq!(
        plugins[0].library_path().unwrap(),
        installed.join("viewer").join(&library),
        "the library of this system"
    );
    // Again: the new version replaces the old one.
    assert_eq!(
        install_into(&file, &installed),
        Ok(Installed::Now("Viewer".into()))
    );
    assert_eq!(uninstall_from("viewer", &installed), Ok(true));
    assert!(load_with(&project, &installed).0.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_plugin_file_must_be_safe_and_fit_this_system() {
    let dir = temp("checks");
    let installed = dir.join("installed");
    let manifest: &[u8] =
        br#"{ "id": "viewer", "name": "Viewer", "version": "1", "library": "viewer", "abi": 2 }"#;
    // Made for another interface (the plugins of Atelier 0.2 say none).
    let old: &[u8] = br#"{ "id": "viewer", "name": "Viewer", "version": "1", "library": "viewer", "atelier": "0.2.0" }"#;
    let file = package(&dir, &[("plugin.json", old)]);
    assert!(install_into(&file, &installed).unwrap_err().contains('2'));
    // No library for this system.
    let file = package(
        &dir,
        &[("plugin.json", manifest), ("other-os/viewer.bin", b"x")],
    );
    assert!(
        install_into(&file, &installed)
            .unwrap_err()
            .contains(&platform())
    );
    // A file out of the folder of the plugin.
    let library = format!(
        "{}viewer{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let file = package(
        &dir,
        &[
            ("plugin.json", manifest),
            (&library, b"x"),
            ("../escape.txt", b"x"),
        ],
    );
    assert!(install_into(&file, &installed).is_err());
    assert!(!dir.join("escape.txt").exists());
    // Not a zip, and an identifier that would leave the folder.
    fs::write(dir.join("text.atelier-plugin"), "not a zip").unwrap();
    assert!(install_into(&dir.join("text.atelier-plugin"), &installed).is_err());
    let file = package(
        &dir,
        &[(
            "plugin.json",
            br#"{ "id": "../evil", "name": "Evil", "version": "1" }"#,
        )],
    );
    assert!(install_into(&file, &installed).is_err());
    let _ = fs::remove_dir_all(&dir);
}
