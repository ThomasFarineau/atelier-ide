//! Languages of the interface, by their identifier: `fr_FR`, `en_US`.
//!
//! The texts live in `locales/<identifier>.json` files: the one at the root of the project for
//! the IDE, and the `locales` folder of each plugin or language for its own texts
//! (`src/plugins/builtins/git/locales/en_US.json`), under keys prefixed by its name (`git.`).
//! `build.rs` embeds every one of them; a language is there as soon as its files are, and its
//! `language` key is the name shown in the settings.
//!
//! In the code a text is named by its key: `tr!("save")`, or `trf!("files", count = n)` for a
//! text with `{count}` in it. A key kept in a table (`const`) is a [`Text`], read with
//! [`Text::get`]. The language is global: changing it redraws the whole interface in the other.

use std::{
    collections::HashMap,
    fmt::Display,
    sync::{
        LazyLock,
        atomic::{AtomicUsize, Ordering},
    },
};

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

/// The language the IDE falls back to for a key another language lacks.
pub const DEFAULT: &str = "en_US";

struct Catalog {
    identifier: &'static str,
    texts: HashMap<&'static str, &'static str>,
}

/// One catalog per language, merged from all its files and sorted by identifier.
static CATALOGS: LazyLock<Vec<Catalog>> = LazyLock::new(|| {
    let mut catalogs: Vec<Catalog> = Vec::new();
    for (identifier, source) in LOCALES {
        let texts: HashMap<String, String> = serde_json::from_str(source)
            .unwrap_or_else(|e| panic!("locales/{identifier}.json: {e}"));
        let index = match catalogs.iter().position(|c| c.identifier == *identifier) {
            Some(index) => index,
            None => {
                catalogs.push(Catalog {
                    identifier,
                    texts: HashMap::new(),
                });
                catalogs.len() - 1
            }
        };
        for (key, text) in texts {
            let key: &'static str = Box::leak(key.into_boxed_str());
            let text: &'static str = Box::leak(text.into_boxed_str());
            catalogs[index].texts.insert(key, text);
        }
    }
    catalogs.sort_by_key(|c| c.identifier);
    catalogs
});

static CURRENT: AtomicUsize = AtomicUsize::new(usize::MAX);

fn catalog(identifier: &str) -> Option<&'static Catalog> {
    CATALOGS.iter().find(|c| c.identifier == identifier)
}

/// The identifiers of the languages there are files for, sorted.
pub fn identifiers() -> Vec<&'static str> {
    CATALOGS.iter().map(|c| c.identifier).collect()
}

/// The identifier of the language of the interface.
pub fn current() -> &'static str {
    // The tests check French texts: they run in French unless one sets a language.
    let start = if cfg!(test) { "fr_FR" } else { DEFAULT };
    CATALOGS
        .get(CURRENT.load(Ordering::Relaxed))
        .or_else(|| catalog(start))
        .map_or(DEFAULT, |c| c.identifier)
}

/// Switches the interface to a language; an unknown identifier is the default language.
pub fn set(identifier: &str) {
    let index = CATALOGS
        .iter()
        .position(|c| c.identifier == identifier)
        .unwrap_or(usize::MAX);
    CURRENT.store(index, Ordering::Relaxed);
}

/// The name of a language, written in that language (`Français`).
pub fn name(identifier: &str) -> &'static str {
    catalog(identifier)
        .and_then(|c| c.texts.get("language").copied())
        .unwrap_or("?")
}

/// The language of the system, among those there are files for: the same identifier
/// (`fr-FR` → `fr_FR`), else the same language (`fr-CA` → `fr_FR`), else the default one.
pub fn system() -> &'static str {
    let locale = sys_locale::get_locale()
        .unwrap_or_default()
        .replace('-', "_");
    let language = locale.split('_').next().unwrap_or_default().to_lowercase();
    CATALOGS
        .iter()
        .find(|c| c.identifier.eq_ignore_ascii_case(&locale))
        .or_else(|| {
            CATALOGS
                .iter()
                .find(|c| c.identifier.split('_').next() == Some(language.as_str()))
        })
        .map_or(DEFAULT, |c| c.identifier)
}

/// The text of a key in the language of the interface, else in the default language, else the
/// key itself.
pub fn t(key: &'static str) -> &'static str {
    [current(), DEFAULT]
        .into_iter()
        .filter_map(catalog)
        .find_map(|c| c.texts.get(key).copied())
        .unwrap_or(key)
}

/// The text of a key made at run time (`html.element.div`), in the language of the interface,
/// else in the default language; None when no catalog has it.
pub fn lookup(key: &str) -> Option<&'static str> {
    [current(), DEFAULT]
        .into_iter()
        .filter_map(catalog)
        .find_map(|c| c.texts.get(key).copied())
}

/// Puts the values in the `{name}` of a text; `{{` and `}}` are braces.
pub fn fill(text: &str, values: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let Some(end) = tail.find('}').filter(|_| tail.starts_with('{')) {
            let name = &tail[1..end];
            match values.iter().find(|(n, _)| *n == name) {
                Some((_, value)) => out.push_str(&value.to_string()),
                None => out.push_str(&tail[..=end]),
            }
            rest = &tail[end + 1..];
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// A shortcut written with the English names of the keys (`Ctrl+Shift+F`), as the language
/// of the interface names them (`Ctrl+Maj+F`).
pub fn shortcut(keys: &str) -> String {
    keys.split('+')
        .map(|key| match key.trim() {
            "Shift" => t("key.shift"),
            "Enter" => t("key.enter"),
            "Esc" => t("key.escape"),
            "Space" => t("key.space"),
            "Del" => t("key.delete"),
            "Click" => t("key.click"),
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The message of an error notice (`"Erreur : …"`, `"Error: …"`), without that prefix.
pub fn error_message(notice: &str) -> Option<&str> {
    CATALOGS
        .iter()
        .filter_map(|c| c.texts.get("error_prefix"))
        .find_map(|prefix| notice.strip_prefix(prefix))
}

pub fn is_error(notice: &str) -> bool {
    error_message(notice).is_some()
}

/// The key of a text kept in a table built at compile time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text(pub &'static str);

impl Text {
    pub fn get(self) -> &'static str {
        t(self.0)
    }
}

/// The text of a key in the language of the interface: `tr!("save")`.
#[macro_export]
macro_rules! tr {
    ($key:literal $(,)?) => {
        $crate::i18n::t($key)
    };
}

/// A text with values in it: `trf!("files", count = n)` for `"{count} files"`.
#[macro_export]
macro_rules! trf {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::t($key),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*],
        )
    };
}

#[cfg(test)]
#[path = "../test/i18n/tests.rs"]
mod tests;
