//! File-type icons (SVG, embedded in the binary, rasterized by egui_extras).

use eframe::egui::{ImageSource, include_image};
use std::path::Path;

pub fn folder(open: bool) -> ImageSource<'static> {
    if open {
        include_image!("../assets/icons/folder-open.svg")
    } else {
        include_image!("../assets/icons/folder.svg")
    }
}

pub fn file(path: &Path) -> ImageSource<'static> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match name.as_str() {
        "cargo.toml" => return include_image!("../assets/icons/cargo.svg"),
        ".gitignore" | ".gitattributes" | ".gitmodules" => {
            return include_image!("../assets/icons/git.svg");
        }
        _ if name.ends_with(".lock") || name == "package-lock.json" => {
            return include_image!("../assets/icons/lock.svg");
        }
        _ => {}
    }
    let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    match extension {
        "rs" => include_image!("../assets/icons/rust.svg"),
        "py" | "pyw" | "pyi" => include_image!("../assets/icons/python.svg"),
        "java" | "jar" | "class" => include_image!("../assets/icons/java.svg"),
        "js" | "mjs" | "cjs" | "jsx" => include_image!("../assets/icons/javascript.svg"),
        "ts" | "tsx" | "mts" | "cts" => include_image!("../assets/icons/typescript.svg"),
        "html" | "htm" => include_image!("../assets/icons/html.svg"),
        "css" | "scss" | "sass" | "less" => include_image!("../assets/icons/css.svg"),
        "json" | "jsonc" => include_image!("../assets/icons/json.svg"),
        "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "env" => {
            include_image!("../assets/icons/config.svg")
        }
        "md" | "markdown" => include_image!("../assets/icons/markdown.svg"),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" => {
            include_image!("../assets/icons/image.svg")
        }
        "sh" | "bash" | "zsh" | "ps1" | "bat" | "cmd" => {
            include_image!("../assets/icons/shell.svg")
        }
        _ => include_image!("../assets/icons/file.svg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_names_win_over_extensions() {
        assert_eq!(file(Path::new("Cargo.toml")).uri(), cargo_uri());
        assert_eq!(file(Path::new("Cargo.lock")).uri(), lock_uri());
        assert_eq!(file(Path::new("a/main.RS")).uri(), rust_uri());
        assert_eq!(file(Path::new("README")).uri(), file_uri());
    }
    fn cargo_uri() -> Option<&'static str> {
        include_image!("../assets/icons/cargo.svg").uri()
    }
    fn lock_uri() -> Option<&'static str> {
        include_image!("../assets/icons/lock.svg").uri()
    }
    fn rust_uri() -> Option<&'static str> {
        include_image!("../assets/icons/rust.svg").uri()
    }
    fn file_uri() -> Option<&'static str> {
        include_image!("../assets/icons/file.svg").uri()
    }

    /// Every embedded icon must parse, otherwise egui shows an error glyph at runtime.
    #[test]
    fn all_icons_are_valid_svg() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let bytes = std::fs::read(entry.path()).unwrap();
            let image = egui_extras::image::load_svg_bytes(&bytes, &Default::default());
            assert!(
                image.is_ok(),
                "{}: {:?}",
                entry.path().display(),
                image.err()
            );
        }
    }
}
