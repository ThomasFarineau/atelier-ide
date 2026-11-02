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
