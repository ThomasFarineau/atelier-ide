//! Live templates and Surround With templates of the user, from `templates.json` of the data folder
//! of Atelier:
//!
//! ```json
//! [
//!   {"abbreviation": "fori", "description": "for loop", "languages": ["js", "ts"],
//!    "text": "for (let i = 0; i < $END$; i++) {\n}"},
//!   {"description": "try / catch", "languages": ["js"], "text": "try {\n$SELECTION$\n} catch (error) {\n}"}
//! ]
//! ```
//!
//! A template with `$SELECTION$` surrounds the selected code (Ctrl+Alt+T); the others are offered
//! by the completion when their abbreviation is typed. `$END$` is where the caret goes. No
//! languages means all of them.

use serde::Deserialize;
use std::{
    fs,
    path::Path,
    sync::OnceLock,
};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Template {
    pub abbreviation: String,
    pub description: String,
    pub languages: Vec<String>,
    pub text: String,
}

impl Template {
    /// Whether it is for a language as `project::language` names it (`rs`, `js`).
    pub fn is_for(&self, language: &str) -> bool {
        self.languages.is_empty() || self.languages.iter().any(|l| l == language)
    }

    /// It surrounds the selection rather than being typed.
    pub fn surrounds(&self) -> bool {
        self.text.contains("$SELECTION$")
    }

    /// The text to insert and where the caret goes in it, with the markers taken out.
    pub fn expand(&self, selection: &str) -> (String, Option<usize>) {
        let text = self.text.replace("$SELECTION$", selection);
        match text.find("$END$") {
            Some(at) => (text.replacen("$END$", "", 1).replace("$END$", ""), Some(at)),
            None => (text, None),
        }
    }
}

pub fn parse(text: &str) -> Vec<Template> {
    serde_json::from_str(text).unwrap_or_default()
}

pub fn load(path: &Path) -> Vec<Template> {
    fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// The templates of the user, read once.
pub fn user() -> &'static [Template] {
    static TEMPLATES: OnceLock<Vec<Template>> = OnceLock::new();
    TEMPLATES.get_or_init(|| load(&crate::cli::data_dir().join("templates.json")))
}

#[cfg(test)]
#[path = "../../test/languages/templates/tests.rs"]
mod tests;
