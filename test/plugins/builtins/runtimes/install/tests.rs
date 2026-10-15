use super::*;

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-runtimes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

#[test]
fn versions_compare_by_their_numbers() {
    assert!(version_key("3.12.10") > version_key("3.12.9"));
    assert!(version_key("21.0.5+11") > version_key("21.0.5+9"));
    assert!(version_key("22.1.0") > version_key("20.18.1"));
}

#[test]
fn pin_files_give_a_version() {
    assert_eq!(parse_pin(".python-version", "3.12\n"), Some("3.12".into()));
    assert_eq!(
        parse_pin(".nvmrc", "# node\nv22.11.0\n"),
        Some("22.11.0".into())
    );
    assert_eq!(
        parse_pin(".sdkmanrc", "# sdkman\njava=21.0.2-tem\n"),
        Some("21.0.2".into())
    );
    assert_eq!(parse_pin(".sdkmanrc", "gradle=8.5\n"), None);
    assert_eq!(parse_pin(".java-version", "\n"), None);
}

#[test]
fn a_pinned_version_is_a_prefix_of_whole_numbers() {
    assert!(satisfies("3.12", "3.12.15", false));
    assert!(!satisfies("3.1", "3.12.15", false));
    assert!(satisfies("v22", "22.11.0", false));
    assert!(satisfies("21", "21.0.12.1+1", false));
    assert!(satisfies("8", "8u432-b06", false));
    assert!(satisfies("lts/*", "22.11.0", true));
    assert!(!satisfies("lts/iron", "23.0.0", false));
    assert!(satisfies("3.12.15", "3.12.15", false));
    assert!(!satisfies("", "3.12.15", false));
}

#[test]
fn python_offers_are_the_stable_full_archives_of_the_system() {
    let sums = "\
aa  cpython-3.12.15+20261009-x86_64-pc-windows-msvc-install_only.tar.gz
bb  cpython-3.12.15+20261009-x86_64-pc-windows-msvc-install_only_stripped.tar.gz
cc  cpython-3.14.8+20261009-x86_64-pc-windows-msvc-install_only.tar.gz
dd  cpython-3.14.8+20261009-x86_64-pc-windows-msvc-freethreaded-install_only.tar.gz
ee  cpython-3.15.0a1+20261009-x86_64-pc-windows-msvc-install_only.tar.gz
ff  cpython-3.13.16+20261009-x86_64-unknown-linux-gnu-install_only.tar.gz
";
    let offers = parse_python_sums(sums, "x86_64-pc-windows-msvc");
    let versions: Vec<&str> = offers.iter().map(|o| o.version.as_str()).collect();
    assert_eq!(versions, ["3.14.8", "3.12.15"]);
    assert_eq!(offers[0].sha256.as_deref(), Some("cc"));
    assert_eq!(
        offers[1].url.as_deref(),
        Some(
            "https://github.com/astral-sh/python-build-standalone/releases/download/20261009/cpython-3.12.15%2B20261009-x86_64-pc-windows-msvc-install_only.tar.gz"
        )
    );
}

#[test]
fn node_offers_are_the_newest_of_each_major() {
    let index = r#"[
        {"version":"v23.1.0","lts":false},
        {"version":"v22.11.0","lts":"Jod"},
        {"version":"v23.0.0","lts":false},
        {"version":"v22.10.0","lts":false},
        {"version":"v16.20.2","lts":"Gallium"}
    ]"#;
    let offers = parse_node_index(index);
    let versions: Vec<(&str, bool)> = offers.iter().map(|o| (o.version.as_str(), o.lts)).collect();
    assert_eq!(versions, [("23.1.0", false), ("22.11.0", true)]);
}

#[test]
fn jdk_offers_are_the_feature_releases() {
    let releases = r#"{"available_lts_releases":[17,21],"available_releases":[17,21,22]}"#;
    let offers = parse_jdk_releases(releases);
    let versions: Vec<(&str, bool)> = offers.iter().map(|o| (o.version.as_str(), o.lts)).collect();
    assert_eq!(versions, [("22", false), ("21", true), ("17", true)]);
    assert_eq!(jdk_version("jdk-21.0.5+11"), "21.0.5+11");
    assert_eq!(jdk_version("jdk8u432-b06"), "8u432-b06");
}

#[test]
fn the_active_runtime_goes_first_on_the_path() {
    let python = if cfg!(windows) {
        "python.exe"
    } else {
        "bin/python3"
    };
    let java = if cfg!(windows) {
        "bin/java.exe"
    } else {
        "bin/java"
    };
    let root = project(
        "env",
        &[
            (&format!(".atelier/runtimes/python/3.12.15/{python}"), ""),
            (&format!(".atelier/runtimes/jdk/21.0.5+11/{java}"), ""),
            (".atelier/runtimes/node/22.11.0/README.md", ""),
        ],
    );
    assert_eq!(installed(&root, Kind::Python), ["3.12.15"]);
    assert_eq!(active(&root, Kind::Python), None, "not chosen yet");
    set_active(&root, Kind::Python, Some("3.12.15"));
    set_active(&root, Kind::Jdk, Some("21.0.5+11"));
    // A version whose program is missing is not used.
    set_active(&root, Kind::Node, Some("22.11.0"));
    assert_eq!(active(&root, Kind::Node), None);
    let env = environment(&root);
    let path = &env.iter().find(|(k, _)| k == "PATH").unwrap().1;
    let first = std::env::split_paths(path).next().unwrap();
    assert_eq!(
        first,
        bin_dirs(Kind::Python, &home(&root, Kind::Python, "3.12.15"))[0]
    );
    let java_home = &env.iter().find(|(k, _)| k == "JAVA_HOME").unwrap().1;
    assert_eq!(
        PathBuf::from(java_home),
        home(&root, Kind::Jdk, "21.0.5+11")
    );
    set_active(&root, Kind::Python, None);
    assert_eq!(active(&root, Kind::Python), None);
    assert_eq!(active(&root, Kind::Jdk).as_deref(), Some("21.0.5+11"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_project_without_runtimes_keeps_the_environment() {
    let root = project("none", &[("README.md", "")]);
    assert!(environment(&root).is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_archive_with_one_folder_is_installed_from_inside_it() {
    let root = project("unpack", &[("a/node-v22/node.exe", "x")]);
    assert_eq!(content_root(&root.join("a")), root.join("a/node-v22"));
    let _ = fs::remove_dir_all(root);
}

/// Downloads real runtimes (`ATELIER_TEST_DOWNLOADS=1`): slow, and needs the network.
#[test]
fn runtimes_install_for_real() {
    if std::env::var_os("ATELIER_TEST_DOWNLOADS").is_none() {
        return;
    }
    let root = project("download", &[("README.md", "")]);
    for (kind, wanted, flag) in [
        (Kind::Python, "3.12", "--version"),
        (Kind::Node, "lts/*", "--version"),
        (Kind::Jdk, "21", "-version"),
    ] {
        let offer = catalog(kind)
            .unwrap()
            .into_iter()
            .find(|o| satisfies(wanted, &o.version, o.lts))
            .unwrap();
        let version = install(&root, kind, &offer, &|_, _, _| {}).unwrap();
        set_active(&root, kind, Some(&version));
        let program = program(&root, kind).unwrap();
        let output = std::process::Command::new(&program)
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", program.display());
        println!(
            "{} {version}: {}",
            kind.name(),
            String::from_utf8_lossy(&[output.stdout, output.stderr].concat()).trim()
        );
    }
    assert!(!kind_dir(&root, Kind::Python).join(".download").exists());
    println!("ROOT={}", root.display());
}
