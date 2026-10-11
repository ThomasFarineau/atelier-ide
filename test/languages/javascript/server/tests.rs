use super::*;

#[test]
fn the_project_server_wins_over_the_managed_one() {
    let dir = std::env::temp_dir().join(format!("atelier-js-server-{}", std::process::id()));
    let bin = dir.join("node_modules").join(".bin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&bin).unwrap();
    let name = executable_names("typescript-language-server").remove(0);
    std::fs::write(bin.join(&name), "").unwrap();
    assert_eq!(find_server(&dir), Some(bin.join(&name)));
    let tsserver = dir.join("node_modules").join("typescript").join("lib");
    std::fs::create_dir_all(&tsserver).unwrap();
    std::fs::write(tsserver.join("tsserver.js"), "").unwrap();
    let launch = launch(&dir).unwrap();
    assert_eq!(launch.args, ["--stdio"]);
    assert!(
        launch.options["tsserver"]["path"]
            .as_str()
            .unwrap()
            .ends_with("tsserver.js")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
