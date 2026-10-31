use super::*;

#[test]
fn special_names_win_over_extensions() {
    assert_eq!(
        file_key(Path::new("Cargo.toml")),
        "languages/rust/extensions/cargo.toml"
    );
    assert_eq!(
        file_key(Path::new("Cargo.lock")),
        "file/extensions/lock+package-lock.json"
    );
    assert_eq!(
        file_key(Path::new("a/main.RS")),
        "languages/rust/extensions/rs"
    );
    assert_eq!(file_key(Path::new("README")), "file/file");
    assert_eq!(
        file_key(Path::new("Dockerfile")),
        "file/extensions/dockerfile+containerfile+.dockerignore"
    );
    assert_eq!(
        file_key(Path::new("build.gradle.kts")),
        "file/extensions/kt+kts"
    );
}

/// Every key the table hands out must exist.
#[test]
fn every_file_type_has_an_icon() {
    for name in [
        "a.rs",
        "a.py",
        "a.java",
        "a.js",
        "a.ts",
        "a.tsx",
        "a.vue",
        "a.svelte",
        "a.html",
        "a.css",
        "a.scss",
        "a.go",
        "a.c",
        "a.cpp",
        "a.cs",
        "a.kt",
        "a.gradle",
        "a.rb",
        "a.php",
        "a.dart",
        "a.hs",
        "a.json",
        "a.toml",
        "a.sh",
        "x",
        "Cargo.toml",
        "Dockerfile",
        "package.json",
        "a.lock",
    ] {
        for dark in [false, true] {
            let _ = file(Path::new(name), dark);
        }
    }
    let _ = (folder(true, true), folder(false, false));
}

/// Every embedded icon must parse, otherwise egui shows an error glyph at runtime.
#[test]
fn all_icons_are_valid_svg() {
    assert!(crate::icon_set::ICONS.len() > 100);
    for icon in crate::icon_set::ICONS {
        for variant in icon
            .variants
            .iter()
            .flatten()
            .filter(|v| v.uri.ends_with(".svg"))
        {
            let image = egui_extras::image::load_svg_bytes(variant.bytes, &Default::default());
            assert!(image.is_ok(), "{}: {:?}", variant.uri, image.err());
        }
    }
}
