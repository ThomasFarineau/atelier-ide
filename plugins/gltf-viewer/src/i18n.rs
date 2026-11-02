//! The texts of the viewer, in the language of the IDE: `tr!` and `trf!`, as in Atelier.
//!
//! The language is the one the IDE gave at the last frame drawn: the model is read on a thread
//! of its own and the shaders built on the one of OpenGL, which have no host to ask.

use atelier_plugin_api::Locales;
use std::sync::{LazyLock, RwLock};

static LOCALES: LazyLock<Locales> = LazyLock::new(|| {
    Locales::new(&[
        ("en_US", include_str!("../locales/en_US.json")),
        ("fr_FR", include_str!("../locales/fr_FR.json")),
    ])
});

static LANGUAGE: RwLock<String> = RwLock::new(String::new());

pub fn set_language(language: &str) {
    if *LANGUAGE.read().unwrap() != language {
        *LANGUAGE.write().unwrap() = language.to_string();
    }
}

pub fn text(key: &str) -> String {
    LOCALES.get(&LANGUAGE.read().unwrap(), key).to_string()
}

pub fn format(key: &str, values: &[(&str, &dyn std::fmt::Display)]) -> String {
    LOCALES.format(&LANGUAGE.read().unwrap(), key, values)
}

macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::text($key)
    };
}

macro_rules! trf {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::format($key, &[$((stringify!($name), &$value as &dyn std::fmt::Display)),*])
    };
}
