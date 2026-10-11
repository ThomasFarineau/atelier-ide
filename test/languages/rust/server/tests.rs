use super::*;

#[test]
fn crates_below_the_root_are_linked() {
    let dir = std::env::temp_dir().join(format!("atelier-rust-server-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for crate_dir in ["tools/cli", "web/target/x", "lib"] {
        std::fs::create_dir_all(dir.join(crate_dir)).unwrap();
        std::fs::write(dir.join(crate_dir).join("Cargo.toml"), "").unwrap();
    }
    std::fs::create_dir_all(dir.join("lib/inner")).unwrap();
    std::fs::write(dir.join("lib/inner/Cargo.toml"), "").unwrap();
    let linked = linked_projects(&dir);
    assert_eq!(
        linked,
        [dir.join("lib/Cargo.toml"), dir.join("tools/cli/Cargo.toml")],
        "target folders are skipped, and a crate's own members are its business"
    );
    assert_eq!(options(&dir)["linkedProjects"].as_array().unwrap().len(), 2);
    std::fs::write(dir.join("Cargo.toml"), "").unwrap();
    assert!(options(&dir).get("linkedProjects").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
