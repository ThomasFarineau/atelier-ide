//! Native plugins: external plugins whose manifest names a dynamic library (`library`). Once
//! the plugin is turned on in the project, the library is loaded and the plugin it declares
//! ([`atelier_plugin_api::declare_plugin!`]) joins the built-in ones, as a [`NativePlugin`].
//!
//! The library shares Rust types with the IDE, so it must have been built like it: the same
//! compiler, target and profile, the same versions and features of egui. Its
//! [`api::Declaration`] says how it was built, and is checked before anything else of the
//! library is called. A library is never unloaded: what it drew may still be painted, and its
//! code is behind the vtables the IDE keeps; turning the plugin on again reuses it.

use super::{
    Plugin as Manifest,
    builtin::{BuiltinPlugin, EditorLayout},
};
use atelier_plugin_api as api;
use eframe::egui;
use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{CStr, c_void},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

type Create = extern "C" fn() -> *mut c_void;

/// The libraries loaded, by file: what makes their plugin.
static LOADED: LazyLock<Mutex<HashMap<PathBuf, Create>>> = LazyLock::new(Default::default);

/// Loads the library of `manifest` (once) and makes its plugin.
pub fn load(manifest: &Manifest) -> Result<NativePlugin, String> {
    let path = manifest
        .library_path()
        .ok_or_else(|| tr!("invalid_library_name").to_string())?;
    let create = {
        let mut loaded = LOADED.lock().unwrap();
        match loaded.get(&path) {
            Some(create) => *create,
            None => {
                let create = open(&path)?;
                loaded.insert(path, create);
                create
            }
        }
    };
    // SAFETY: `create` gives a `Box<Box<dyn Plugin>>` as a pointer (`declare_plugin!`), of
    // the same types as the IDE's: `open` checked the library was built like it.
    let plugin = unsafe { *Box::from_raw(create().cast::<Box<dyn api::Plugin>>()) };
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    Ok(NativePlugin {
        id: leak(&manifest.id),
        name: leak(&manifest.name),
        description: leak(&manifest.description),
        version: leak(&manifest.version),
        plugin,
    })
}

/// Opens a library and checks how it was built.
fn open(path: &Path) -> Result<Create, String> {
    if !path.is_file() {
        return Err(trf!("native_library_missing", path = path.display()));
    }
    let failed =
        |e: &dyn std::fmt::Display| trf!("native_library_error", path = path.display(), e = e);
    // SAFETY: loading a library runs its initializers: the user turned the plugin on, and the
    // Plugins window tells that a native plugin runs inside the IDE.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| failed(&e))?;
    // SAFETY: the symbol is the `Declaration` static of `declare_plugin!`, which stays valid
    // while the library is loaded. Only `abi` and `rustc`, laid out as in C, are read before
    // the library is known to be built like the IDE.
    let declaration: &api::Declaration = unsafe {
        let symbol = library
            .get::<*const api::Declaration>(api::SYMBOL)
            .map_err(|e| failed(&e))?;
        &**symbol
    };
    let built = if declaration.abi == api::ABI {
        // SAFETY: a C string of the library, with the same ABI.
        unsafe { CStr::from_ptr(declaration.rustc) }
            .to_string_lossy()
            .into_owned()
    } else {
        format!("ABI {}", declaration.abi)
    };
    let mine = api::RUSTC.trim_end_matches('\0');
    if built != mine || (declaration.fingerprint)() != api::fingerprint() {
        return Err(trf!(
            "native_library_mismatch",
            path = path.display(),
            built = built,
            mine = mine
        ));
    }
    let create = declaration.create;
    // Kept loaded for the life of the IDE (see the module).
    std::mem::forget(library);
    Ok(create)
}

/// A native plugin, as the IDE sees its built-in plugins.
pub struct NativePlugin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    version: &'static str,
    plugin: Box<dyn api::Plugin>,
}

/// What the IDE gives a native plugin while it draws.
struct Host {
    theme: crate::ui::Theme,
}

impl api::Host for Host {
    fn language(&self) -> &str {
        crate::i18n::current()
    }

    fn theme(&self) -> api::Theme {
        let t = &self.theme;
        api::Theme {
            dark: t.dark,
            island: t.island,
            separator: t.separator,
            accent: t.accent,
            text: t.text,
            text_secondary: t.text_secondary,
            error: t.error,
        }
    }

    fn icon_toggle(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        selected: bool,
        tooltip: &str,
    ) -> egui::Response {
        crate::ui::icon_toggle(ui, icon, selected, tooltip)
    }
}

impl BuiltinPlugin for NativePlugin {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn version(&self) -> &'static str {
        self.version
    }
    fn external(&self) -> bool {
        true
    }

    fn opens_binary(&self, path: &Path) -> bool {
        self.plugin.opens_binary(path)
    }
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        self.plugin.editor_layout(path).map(|layout| match layout {
            api::Layout::Text => EditorLayout::Text,
            api::Layout::Split => EditorLayout::Split,
            api::Layout::Preview => EditorLayout::Preview,
        })
    }
    fn layout_switch(&self, path: &Path) -> bool {
        self.plugin.layout_switch(path)
    }
    fn file_image(&self, path: &Path, _dark: bool) -> Option<egui::ImageSource<'static>> {
        let icon = self.plugin.file_icon(path)?;
        Some(egui::ImageSource::Bytes {
            uri: Cow::Borrowed(icon.uri),
            bytes: egui::load::Bytes::Static(icon.svg),
        })
    }
    fn file_closed(&mut self, ctx: &egui::Context, path: &Path) {
        self.plugin.file_closed(ctx, path);
    }
    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, _scroll: Option<f32>) {
        let host = Host {
            theme: crate::ui::theme(ui.ctx()),
        };
        self.plugin.preview(ui, path, text, &host);
    }
}

#[cfg(test)]
#[path = "../../test/plugins/native/tests.rs"]
mod tests;
