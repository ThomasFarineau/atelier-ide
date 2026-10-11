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
#[path = "../test/icons/tests.rs"]
mod tests;
