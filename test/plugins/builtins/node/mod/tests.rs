use super::*;

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "atelier-node-{}-{}",
        std::process::id(),
        files.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

#[test]
fn the_manager_comes_from_the_field_then_the_lockfile() {
    let root = project(&[
        (
            "package.json",
            r#"{"name":"root","workspaces":["apps/*"],"scripts":{"dev":"vite","build":"vite build"},"dependencies":{"vite":"5"}}"#,
        ),
        ("pnpm-lock.yaml", ""),
        (
            "apps/web/package.json",
            r#"{"name":"web","scripts":{"test":"vitest"}}"#,
        ),
        (
            "tools/bun/package.json",
            r#"{"name":"b","packageManager":"bun@1.1.0"}"#,
        ),
        ("tools/node_modules/x/package.json", r#"{"name":"hidden"}"#),
    ]);
    let packages = scan(&root);
    let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["root", "web", "b"], "node_modules is skipped");
    assert_eq!(packages[0].manager, Manager::Pnpm);
    assert_eq!(packages[0].rel, "package.json");
    assert_eq!(
        packages[1].manager,
        Manager::Pnpm,
        "the lockfile of the workspace root"
    );
    assert_eq!(packages[1].rel, "apps/web/package.json");
    assert_eq!(
        packages[2].manager,
        Manager::Bun,
        "the packageManager field wins"
    );
    assert_eq!(
        packages[0].scripts,
        [
            ("dev".to_string(), "vite".to_string()),
            ("build".to_string(), "vite build".to_string())
        ],
        "the order of the file"
    );
    assert!(
        packages[0].missing_modules,
        "vite is declared and not installed"
    );
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    assert!(!scan(&root)[0].missing_modules);
    assert_eq!(script_line(&root, "build"), 0, "a one-line file");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn commands_run_from_the_folder_of_the_package() {
    let root = Path::new("/p");
    let here = crate::plugins::command_in(root, root, "npm", &["run".into(), "dev".into()]);
    assert!(here.ends_with("'npm' 'run' 'dev'"), "{here}");
    let sub = crate::plugins::command_in(&root.join("apps/web"), root, "pnpm", &["install".into()]);
    if cfg!(windows) {
        assert!(sub.starts_with("Set-Location -LiteralPath '"), "{sub}");
    } else {
        assert!(sub.starts_with("cd '/p/apps/web' && "), "{sub}");
    }
    assert_eq!(Manager::Bun.program(), "bun");
    assert_eq!(
        Manager::Yarn.program(),
        if cfg!(windows) { "yarn.cmd" } else { "yarn" }
    );
}

#[test]
fn the_plugin_applies_only_where_there_is_a_package_json() {
    let dir = std::env::temp_dir().join(format!("atelier-node-none-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut plugin = NodePlugin {
        packages: scan(&dir),
        ..Default::default()
    };
    assert!(!plugin.applies());
    std::fs::write(dir.join("package.json"), r#"{"name":"x"}"#).unwrap();
    plugin.packages = scan(&dir);
    assert!(plugin.applies());
    let _ = std::fs::remove_dir_all(&dir);
}
