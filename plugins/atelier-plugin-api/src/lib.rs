//! The interface between Atelier and its native plugins.
//!
//! A native plugin is a dynamic library (`.dll`, `.so`, `.dylib`) that implements [`Plugin`] and
//! exports it with [`declare_plugin!`]. The IDE loads it when the plugin is turned on in a
//! project (see the `library` field of the manifests of `.atelier/plugins`), then draws it in
//! its editor tabs like a built-in plugin.
//!
//! The library and the IDE share Rust types (`egui::Ui`, trait objects): there is no stable ABI
//! between them, so they must be built by the same compiler, for the same target and profile,
//! from the same versions and features of their crates. The IDE checks it before it calls
//! anything in the library ([`Declaration`]) and refuses a library built otherwise: build the
//! plugin in the workspace of the IDE, with the same `cargo build` options.

use std::{
    any::TypeId,
    collections::HashMap,
    ffi::{c_char, c_void},
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

pub use eframe::{egui, egui_glow, glow};

/// Changes when [`Declaration`] does.
pub const ABI: u32 = 1;

/// The name of the symbol of the [`Declaration`] in a library.
pub const SYMBOL: &[u8] = b"ATELIER_PLUGIN\0";

/// The compiler, target and profile this crate was built with, as a C string.
#[doc(hidden)]
pub const RUSTC: &str = concat!(env!("ATELIER_RUSTC"), "\0");

/// How the editor shows a document: source, source and preview side by side, or preview only.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Layout {
    Text,
    #[default]
    Split,
    Preview,
}

/// The colors of the interface, for a plugin that draws itself.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Theme {
    pub dark: bool,
    /// Tool windows and editor.
    pub island: egui::Color32,
    pub separator: egui::Color32,
    pub accent: egui::Color32,
    pub text: egui::Color32,
    pub text_secondary: egui::Color32,
    pub error: egui::Color32,
}

/// An icon given to files: an SVG kept in the library, with a URI that ends in `.svg` and is
/// unique to the plugin (`bytes://gltf-viewer/model.svg`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileIcon {
    pub uri: &'static str,
    pub svg: &'static [u8],
}

/// What a plugin may ask the IDE while it draws.
pub trait Host {
    /// The identifier of the language of the interface (`en_US`, `fr_FR`).
    fn language(&self) -> &str;
    fn theme(&self) -> Theme;
    /// A button of the toolbars of the IDE, with an icon of its set (`run`, `refresh`…).
    fn icon_toggle(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        selected: bool,
        tooltip: &str,
    ) -> egui::Response;
    fn icon_button(&self, ui: &mut egui::Ui, icon: &str, tooltip: &str) -> egui::Response {
        self.icon_toggle(ui, icon, false, tooltip)
    }
}

/// A native plugin. Every method has a default: a plugin implements what it uses.
#[allow(unused_variables)]
pub trait Plugin {
    /// True when the plugin displays this file itself: it is opened without being read as text.
    fn opens_binary(&self, path: &Path) -> bool {
        false
    }
    /// Some when the plugin previews this document.
    fn editor_layout(&self, path: &Path) -> Option<Layout> {
        None
    }
    /// False when the layout switch makes no sense for this document (e.g. an image).
    fn layout_switch(&self, path: &Path) -> bool {
        true
    }
    /// The icon of the files this plugin handles, in the project tree and the tabs.
    fn file_icon(&self, path: &Path) -> Option<FileIcon> {
        None
    }
    /// Draws the preview of a document. The preview may edit the text.
    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, host: &dyn Host) {}
    /// The last tab of `path` closed: what the plugin kept for it goes.
    fn file_closed(&mut self, ctx: &egui::Context, path: &Path) {}
}

/// What a library exports under [`SYMBOL`]. Only its first two fields are read before the IDE
/// knows the library was built like itself.
#[repr(C)]
pub struct Declaration {
    pub abi: u32,
    /// [`RUSTC`] of the library.
    pub rustc: *const c_char,
    /// [`fingerprint`] of the library.
    pub fingerprint: extern "C" fn() -> u64,
    /// A new plugin: a `Box<Box<dyn Plugin>>` turned into a pointer.
    pub create: extern "C" fn() -> *mut c_void,
}

// SAFETY: the pointers are to constants of the library.
unsafe impl Sync for Declaration {}

/// The identity of the types the IDE and a plugin share. Two builds give the same number only
/// if they used the same versions and features of these crates, the same compiler and profile.
pub fn fingerprint() -> u64 {
    let mut hasher = DefaultHasher::new();
    for id in [
        TypeId::of::<dyn Plugin>(),
        TypeId::of::<dyn Host>(),
        TypeId::of::<egui::Context>(),
        TypeId::of::<egui::Ui>(),
        TypeId::of::<egui::Response>(),
        TypeId::of::<egui_glow::CallbackFn>(),
        TypeId::of::<glow::Context>(),
    ] {
        id.hash(&mut hasher);
    }
    hasher.finish()
}

/// Exports a plugin from a library: `declare_plugin!(MyPlugin::default);`, with a function or
/// closure that makes it.
#[macro_export]
macro_rules! declare_plugin {
    ($make:expr) => {
        extern "C" fn __atelier_fingerprint() -> u64 {
            $crate::fingerprint()
        }

        extern "C" fn __atelier_create() -> *mut ::std::ffi::c_void {
            let plugin: ::std::boxed::Box<dyn $crate::Plugin> = ::std::boxed::Box::new(($make)());
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(plugin)).cast()
        }

        #[unsafe(no_mangle)]
        pub static ATELIER_PLUGIN: $crate::Declaration = $crate::Declaration {
            abi: $crate::ABI,
            rustc: $crate::RUSTC.as_ptr().cast(),
            fingerprint: __atelier_fingerprint,
            create: __atelier_create,
        };
    };
}

/// The translations of a plugin: the `locales/<identifier>.json` files it embeds, each a flat
/// object of keys and texts. A text missing in a language is the English one, else the key.
pub struct Locales {
    catalogs: HashMap<&'static str, HashMap<String, String>>,
}

impl Locales {
    /// `files` pairs an identifier (`en_US`) with the JSON of its file; a file that is not a
    /// flat object of texts is left out.
    pub fn new(files: &[(&'static str, &str)]) -> Self {
        let catalogs = files
            .iter()
            .map(|(id, json)| (*id, serde_json::from_str(json).unwrap_or_default()))
            .collect();
        Self { catalogs }
    }

    pub fn get<'a>(&'a self, language: &str, key: &'a str) -> &'a str {
        let texts = |l: &str| self.catalogs.get(l).and_then(|c| c.get(key));
        texts(language)
            .or_else(|| texts("en_US"))
            .map_or(key, String::as_str)
    }

    /// The text of `key` with each `{name}` replaced by its value.
    pub fn format(
        &self,
        language: &str,
        key: &str,
        values: &[(&str, &dyn std::fmt::Display)],
    ) -> String {
        let mut text = self.get(language, key).to_string();
        for (name, value) in values {
            text = text.replace(&format!("{{{name}}}"), &value.to_string());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_fall_back_to_english_then_to_the_key() {
        let locales = Locales::new(&[
            (
                "en_US",
                r#"{ "a": "One", "b": "Two {n}", "q": "Say \"hi\"\n" }"#,
            ),
            ("fr_FR", r#"{ "a": "Un" }"#),
        ]);
        assert_eq!(locales.get("fr_FR", "a"), "Un");
        assert_eq!(locales.get("fr_FR", "b"), "Two {n}");
        assert_eq!(locales.get("fr_FR", "c"), "c");
        assert_eq!(locales.get("en_US", "q"), "Say \"hi\"\n");
        assert_eq!(locales.format("en_US", "b", &[("n", &3)]), "Two 3");
    }

    #[test]
    fn the_fingerprint_is_stable() {
        assert_eq!(fingerprint(), fingerprint());
        assert!(RUSTC.ends_with('\0') && RUSTC.starts_with("rustc "));
    }
}
