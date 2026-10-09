//! File-type icons: which icon of `assets/icons` a file gets (see `icon_set` for the variants).

use eframe::egui::ImageSource;
use std::path::Path;

pub fn folder(open: bool, dark: bool) -> ImageSource<'static> {
    source(folder_key(open), dark)
}

fn source(key: &str, dark: bool) -> ImageSource<'static> {
    crate::icon_set::source(key, dark).expect("file icon")
}

fn folder_key(open: bool) -> &'static str {
    if open {
        "file/folder-open"
    } else {
        "file/folder"
    }
}

pub fn file(path: &Path, dark: bool) -> ImageSource<'static> {
    source(file_key(path), dark)
}

/// Key of the icon of a file type: the `extensions` files of the languages and of `assets`.
pub fn file_key(path: &Path) -> &'static str {
    crate::icon_set::extension_icon("file", path).unwrap_or("file/file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_names_win_over_extensions() {
        assert_eq!(
            file_key(Path::new("Cargo.toml")),
            "file/extensions/cargo.toml"
        );
        assert_eq!(
            file_key(Path::new("Cargo.lock")),
            "file/extensions/lock+package-lock.json"
        );
        assert_eq!(file_key(Path::new("a/main.RS")), "file/extensions/rs");
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
}
