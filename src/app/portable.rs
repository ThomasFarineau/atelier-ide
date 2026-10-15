//! Export and import of the settings, and the sync folder. A bundle is one JSON file with the
//! options that follow the user from a machine to another (theme, fonts, language, the options of
//! the editor, the plugins that are on), their live templates and their dictionary. What belongs
//! to a machine (recent projects, answers about the MCP server…) stays out of it.
//!
//! The sync folder is any folder that something else carries around (OneDrive, Dropbox, a Git
//! repository): the bundle is written there when the settings change, and read at the start when
//! it is newer than the settings of this machine.

use super::{Draft, Ide};
use anyhow::{Context as _, Result, bail};
use eframe::egui;
use serde_json::{Value, json};
use std::{fs, path::Path, time::SystemTime};

/// The options that go in a bundle.
pub const KEYS: [&str; 13] = [
    "theme",
    "font_size",
    "enabled_plugins",
    "disabled_builtins",
    "wheel_zoom_off",
    "inlay_hints_off",
    "sticky_lines_off",
    "breadcrumbs_off",
    "spellcheck_off",
    "hide_ignored",
    "commit_check_off",
    "fonts",
    "language",
];

/// The name of the bundle in the sync folder.
pub const FILE: &str = "atelier-settings.json";

/// The bundle of the settings (as JSON), the templates (the text of `templates.json`) and the
/// words of the dictionary.
pub fn bundle(settings: &Value, templates: &str, dictionary: &str) -> Value {
    let mut options = serde_json::Map::new();
    for key in KEYS {
        if let Some(value) = settings.get(key) {
            options.insert(key.to_string(), value.clone());
        }
    }
    json!({
        "atelier_settings": 1,
        "settings": options,
        "templates": serde_json::from_str::<Value>(templates).unwrap_or_else(|_| json!([])),
        "dictionary": dictionary.lines().map(str::trim).filter(|w| !w.is_empty()).collect::<Vec<_>>(),
    })
}

/// What a bundle brings: the options put in `settings`, and the texts of the files to write.
pub struct Imported {
    pub templates: Option<String>,
    pub dictionary: Option<String>,
    /// The options that changed.
    pub changed: usize,
}

/// Puts the options of a bundle in `settings`.
pub fn apply(bundle: &Value, settings: &mut Value) -> Result<Imported> {
    if bundle.get("atelier_settings").is_none() {
        bail!(tr!("not_a_settings_file"));
    }
    let mut changed = 0;
    if let (Some(options), Some(target)) = (
        bundle.get("settings").and_then(Value::as_object),
        settings.as_object_mut(),
    ) {
        for key in KEYS {
            if let Some(value) = options.get(key)
                && target.get(key) != Some(value)
            {
                target.insert(key.to_string(), value.clone());
                changed += 1;
            }
        }
    }
    let templates = bundle
        .get("templates")
        .filter(|t| t.as_array().is_some_and(|a| !a.is_empty()))
        .and_then(|t| serde_json::to_string_pretty(t).ok());
    let dictionary = bundle
        .get("dictionary")
        .and_then(Value::as_array)
        .filter(|w| !w.is_empty())
        .map(|words| {
            words
                .iter()
                .filter_map(Value::as_str)
                .map(|w| format!("{w}\n"))
                .collect::<String>()
        });
    Ok(Imported {
        templates,
        dictionary,
        changed,
    })
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Ide {
    /// The bundle of this machine.
    pub(super) fn settings_bundle(&self) -> Value {
        let data = crate::cli::data_dir();
        let templates = fs::read_to_string(data.join("templates.json")).unwrap_or_default();
        let dictionary = fs::read_to_string(data.join("dictionary.txt")).unwrap_or_default();
        bundle(
            &serde_json::to_value(&self.settings).unwrap_or_default(),
            &templates,
            &dictionary,
        )
    }

    /// Writes the bundle to `path`.
    pub(super) fn export_settings_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_string_pretty(&self.settings_bundle())?)
            .with_context(|| path.display().to_string())
    }

    /// Takes the bundle of `path`: the options change at once, the templates and the dictionary
    /// are written. Returns how many options changed.
    pub(super) fn import_settings_from(
        &mut self,
        path: &Path,
        ctx: &egui::Context,
    ) -> Result<usize> {
        let text = fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let bundle: Value = serde_json::from_str(&text).context(tr!("not_a_settings_file"))?;
        let mut settings = serde_json::to_value(&self.settings)?;
        let imported = apply(&bundle, &mut settings)?;
        self.settings = serde_json::from_value(settings).context(tr!("not_a_settings_file"))?;
        let data = crate::cli::data_dir();
        fs::create_dir_all(&data)?;
        if let Some(templates) = &imported.templates {
            fs::write(data.join("templates.json"), templates)?;
        }
        if let Some(words) = &imported.dictionary {
            // The words of the bundle join the ones of this machine.
            let mut all: std::collections::BTreeSet<String> = fs::read_to_string(data.join("dictionary.txt"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect();
            all.extend(words.lines().map(str::to_string));
            fs::write(
                data.join("dictionary.txt"),
                all.into_iter().map(|w| format!("{w}\n")).collect::<String>(),
            )?;
        }
        // What the dialog shows, and what the window does with it.
        self.settings_draft = Draft::of(&self.settings);
        self.settings_original = self.settings_draft.clone();
        if let Some(language) = &self.settings.language {
            crate::i18n::set(language);
        }
        self.apply_theme(ctx);
        self.persist_settings()?;
        ctx.request_repaint();
        Ok(imported.changed)
    }

    /// Writes the bundle to the sync folder, when there is one.
    pub(super) fn sync_out(&mut self) {
        let Some(folder) = self.settings.sync_folder.clone() else {
            return;
        };
        if let Err(e) = self.export_settings_to(&folder.join(FILE)) {
            self.status = trf!("error_detail", e = format!("{e:#}"));
        }
    }

    /// Reads the bundle of the sync folder when it is newer than the settings of this machine.
    pub(super) fn sync_in(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.settings.sync_folder.clone() else {
            return;
        };
        let file = folder.join(FILE);
        let (Some(theirs), Some(ours)) = (modified(&file), modified(&super::settings_path()))
        else {
            return;
        };
        if theirs > ours {
            match self.import_settings_from(&file, ctx) {
                Ok(n) if n > 0 => self.status = trf!("settings_synced", count = n),
                Ok(_) => {}
                Err(e) => self.status = trf!("error_detail", e = format!("{e:#}")),
            }
        }
    }
}

#[cfg(test)]
#[path = "../../test/app/portable/tests.rs"]
mod tests;
