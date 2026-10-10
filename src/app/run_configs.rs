//! Run configurations, as IntelliJ's: named commands of the project, chosen in the run widget of
//! the title bar and started with Shift+F10, each in a terminal tab of its own that the next run
//! takes again. They are kept in `.atelier/run.json`, which can be shared with the project.
//!
//! The commands the Node.js, Maven and Gradle windows run become temporary configurations, as
//! in IntelliJ: they can be run again from the widget, and saved to be kept.

use super::Ide;
use crate::ui;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Where the configurations of a project are kept.
pub const FILE: &str = ".atelier/run.json";
/// The temporary configurations kept, the oldest dropped first.
const MAX_TEMPORARY: usize = 5;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct RunConfig {
    pub name: String,
    /// What is typed in the terminal.
    pub command: String,
    /// The folder it runs in, relative to the project; empty for the project.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dir: String,
    /// Made from a run of a tool window; not saved until the user says so.
    #[serde(skip)]
    pub temporary: bool,
}

impl RunConfig {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            ..Self::default()
        }
    }

    /// The line typed in a terminal started at `root`.
    pub fn line(&self, root: &Path) -> String {
        let dir = self.dir.trim().trim_matches(['/', '\\']);
        if dir.is_empty() {
            return self.command.clone();
        }
        let folder = root.join(dir);
        let folder = folder.to_string_lossy();
        if cfg!(windows) {
            format!(
                "Set-Location -LiteralPath '{}'; {}",
                folder.replace('\'', "''"),
                self.command
            )
        } else {
            format!("cd '{}' && {}", folder.replace('\'', "'\\''"), self.command)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct RunConfigs {
    #[serde(default)]
    pub configurations: Vec<RunConfig>,
    /// The name of the one the widget runs; none: the current file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
}

impl RunConfigs {
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the configurations that are not temporary.
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let kept = RunConfigs {
            configurations: self
                .configurations
                .iter()
                .filter(|c| !c.temporary)
                .cloned()
                .collect(),
            selected: self
                .selected
                .clone()
                .filter(|name| self.get(name).is_some_and(|c| !c.temporary)),
        };
        let path = root.join(FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut text = serde_json::to_string_pretty(&kept).map_err(|e| e.to_string())?;
        text.push('\n');
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn get(&self, name: &str) -> Option<&RunConfig> {
        self.configurations.iter().find(|c| c.name == name)
    }

    pub fn selected(&self) -> Option<&RunConfig> {
        self.get(self.selected.as_deref()?)
    }

    /// A name no configuration has: `name`, else `name (2)`…
    pub fn free_name(&self, name: &str) -> String {
        let mut candidate = name.to_string();
        let mut n = 2;
        while self.get(&candidate).is_some() {
            candidate = format!("{name} ({n})");
            n += 1;
        }
        candidate
    }

    /// The temporary configuration of a run from a tool window, made or taken again, and
    /// selected.
    pub fn temporary(&mut self, name: &str, command: &str) -> RunConfig {
        let same = self
            .configurations
            .iter()
            .position(|c| c.command == command && (c.temporary || c.name == name));
        let config = match same {
            Some(index) => self.configurations[index].clone(),
            None => {
                let config = RunConfig {
                    name: self.free_name(name),
                    command: command.to_string(),
                    dir: String::new(),
                    temporary: true,
                };
                self.configurations.push(config.clone());
                let temporary: Vec<usize> = self
                    .configurations
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.temporary)
                    .map(|(i, _)| i)
                    .collect();
                if temporary.len() > MAX_TEMPORARY {
                    self.configurations.remove(temporary[0]);
                }
                config
            }
        };
        self.selected = Some(config.name.clone());
        config
    }
}

/// The package manager of a Node.js project, from its lock file.
fn node_manager(root: &Path) -> &'static str {
    [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
    ]
    .iter()
    .find(|(lock, _)| root.join(lock).is_file())
    .map_or("npm", |(_, manager)| manager)
}

/// The configurations the project's tools suggest, to add in one click.
pub fn suggestions(root: &Path) -> Vec<RunConfig> {
    let mut out = Vec::new();
    if root.join("Cargo.toml").is_file() {
        for (name, command) in [
            ("cargo run", "cargo run"),
            ("cargo test", "cargo test"),
            ("cargo build", "cargo build"),
            ("cargo clippy", "cargo clippy --all-targets"),
        ] {
            out.push(RunConfig::new(name, command));
        }
    }
    if let Ok(text) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(scripts) = value["scripts"].as_object()
    {
        let manager = node_manager(root);
        for script in scripts.keys() {
            out.push(RunConfig::new(
                format!("{manager} {script}"),
                format!("{manager} run {script}"),
            ));
        }
    }
    if root.join("pom.xml").is_file() {
        let maven = if root
            .join(if cfg!(windows) { "mvnw.cmd" } else { "mvnw" })
            .is_file()
        {
            if cfg!(windows) {
                ".\\mvnw.cmd"
            } else {
                "./mvnw"
            }
        } else {
            "mvn"
        };
        for goal in ["package", "test", "clean install"] {
            out.push(RunConfig::new(
                format!("mvn {goal}"),
                format!("{maven} {goal}"),
            ));
        }
    }
    let gradle_build = [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|f| root.join(f).is_file());
    if gradle_build {
        let gradle = if root.join("gradlew").is_file() || root.join("gradlew.bat").is_file() {
            if cfg!(windows) {
                ".\\gradlew.bat"
            } else {
                "./gradlew"
            }
        } else {
            "gradle"
        };
        for task in ["build", "test", "run"] {
            out.push(RunConfig::new(
                format!("gradle {task}"),
                format!("{gradle} {task}"),
            ));
        }
    }
    out
}

/// The Run/Debug Configurations dialog: a copy of the configurations being edited.
pub(super) struct Dialog {
    configs: Vec<RunConfig>,
    selected: Option<usize>,
    error: Option<String>,
}

impl Ide {
    /// The configurations of the project, read again when the project changed.
    pub(super) fn run_configs(&mut self) -> &mut RunConfigs {
        if self.run_configs.0 != self.root {
            self.run_configs = (self.root.clone(), RunConfigs::load(&self.root));
            self.run_panes.clear();
        }
        &mut self.run_configs.1
    }

    /// Shift+F10: the selected configuration, or the current file.
    pub(super) fn run_selected(&mut self) {
        match self.run_configs().selected().cloned() {
            Some(config) => self.run_config(&config),
            None => self.run_file(),
        }
    }

    /// Runs a configuration in its terminal tab, made the first time.
    pub(super) fn run_config(&mut self, config: &RunConfig) {
        let line = config.line(&self.root);
        self.set_terminal(true);
        let known = self.run_panes.get(&config.name).copied();
        let tab = known.and_then(|id| {
            self.terminals.iter_mut().position(|tab| {
                tab.root
                    .leaves()
                    .iter()
                    .any(|p| p.id == id && !p.term.exited)
            })
        });
        let tab = match tab {
            Some(tab) => tab,
            None => {
                self.start_terminal(None);
                let Some(tab) = self.terminals.last_mut() else {
                    return;
                };
                tab.title = Some(config.name.clone());
                self.run_panes.insert(config.name.clone(), tab.active);
                self.terminals.len() - 1
            }
        };
        self.active_terminal = tab;
        if let Some(pane) = self.terminals[tab].pane() {
            let result = pane.term.command(&line);
            self.report(result);
        }
    }

    /// The pane the selected configuration last ran in, while it is there.
    pub(super) fn run_pane(&self) -> Option<u64> {
        let name = self.run_configs.1.selected.as_ref()?;
        let id = *self.run_panes.get(name)?;
        self.terminals
            .iter()
            .any(|tab| {
                tab.root
                    .leaves()
                    .iter()
                    .any(|p| p.id == id && !p.term.exited)
            })
            .then_some(id)
    }

    /// A run of a tool window (Node.js, Maven, Gradle): a temporary configuration, selected.
    pub(super) fn run_temporary(&mut self, name: &str, command: &str) {
        let config = self.run_configs().temporary(name, command);
        self.run_config(&config);
    }

    pub(super) fn save_run_configs(&mut self) {
        let root = self.root.clone();
        if let Err(e) = self.run_configs().save(&root) {
            self.status = trf!("error", e = e);
        }
    }

    /// The label of the run widget: the selected configuration, or the current file.
    pub(super) fn run_label(&mut self) -> String {
        if let Some(config) = self.run_configs().selected() {
            return config.name.clone();
        }
        match self.documents.get(self.active) {
            Some(doc) => doc.title(),
            None => tr!("current_file").into(),
        }
    }

    /// The configurations in the menu of the run widget, to choose one.
    pub(super) fn run_configs_menu(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui.ctx());
        let configs = self.run_configs().configurations.clone();
        let selected = self.run_configs().selected.clone();
        let mut choose: Option<Option<String>> = None;
        if ui
            .add(egui::Button::selectable(
                selected.is_none(),
                tr!("current_file"),
            ))
            .clicked()
        {
            choose = Some(None);
        }
        for config in &configs {
            let mut text = egui::RichText::new(&config.name);
            if config.temporary {
                text = text.italics().color(t.text_secondary);
            }
            let on = selected.as_deref() == Some(config.name.as_str());
            if ui
                .add(egui::Button::selectable(on, text))
                .on_hover_text(&config.command)
                .clicked()
            {
                choose = Some(Some(config.name.clone()));
            }
        }
        let temporary = self
            .run_configs()
            .selected()
            .filter(|c| c.temporary)
            .map(|c| c.name.clone());
        ui.separator();
        if let Some(name) = temporary
            && ui.button(trf!("save_configuration", name = name)).clicked()
        {
            if let Some(config) = self
                .run_configs()
                .configurations
                .iter_mut()
                .find(|c| c.name == name)
            {
                config.temporary = false;
            }
            self.save_run_configs();
            ui.close();
        }
        if ui.button(tr!("edit_configurations")).clicked() {
            self.open_run_dialog();
            ui.close();
        }
        if let Some(choice) = choose {
            self.run_configs().selected = choice;
            self.save_run_configs();
            ui.close();
        }
    }

    pub(super) fn open_run_dialog(&mut self) {
        let configs = self.run_configs().configurations.clone();
        let selected = self
            .run_configs()
            .selected
            .as_ref()
            .and_then(|name| configs.iter().position(|c| &c.name == name))
            .or((!configs.is_empty()).then_some(0));
        self.run_dialog = Some(Dialog {
            configs,
            selected,
            error: None,
        });
    }

    pub(super) fn run_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.run_dialog.take() else {
            return;
        };
        let t = ui::theme(ctx);
        let suggestions = suggestions(&self.root);
        let mut close = false;
        let mut apply = false;
        let mut run = None;
        egui::Modal::new(egui::Id::new("run_configurations")).show(ctx, |ui| {
            ui.set_width(720.0);
            ui::dialog_title(ui, tr!("run_configurations"));
            ui.horizontal_top(|ui| {
                // The list, with what adds or removes one.
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    ui.horizontal(|ui| {
                        let add = ui::icon_button(ui, "add", tr!("add_configuration"));
                        egui::Popup::menu(&add)
                            .id(egui::Id::new("run_configurations_add"))
                            .show(|ui| {
                                ui.set_min_width(220.0);
                                if ui.button(tr!("shell_command")).clicked() {
                                    let name = free_name(&dialog.configs, tr!("new_configuration"));
                                    dialog.configs.push(RunConfig::new(name, ""));
                                    dialog.selected = Some(dialog.configs.len() - 1);
                                    ui.close();
                                }
                                if !suggestions.is_empty() {
                                    ui.separator();
                                }
                                for suggestion in &suggestions {
                                    if ui.button(&suggestion.name).clicked() {
                                        let mut config = suggestion.clone();
                                        config.name = free_name(&dialog.configs, &config.name);
                                        dialog.configs.push(config);
                                        dialog.selected = Some(dialog.configs.len() - 1);
                                        ui.close();
                                    }
                                }
                            });
                        let some = dialog.selected.is_some();
                        if ui::icon_button_enabled(ui, "remove", some, tr!("remove_configuration"))
                            .clicked()
                            && let Some(index) = dialog.selected
                        {
                            dialog.configs.remove(index);
                            dialog.selected = (!dialog.configs.is_empty())
                                .then(|| index.min(dialog.configs.len() - 1));
                        }
                        if ui::icon_button_enabled(ui, "copy", some, tr!("copy_configuration"))
                            .clicked()
                            && let Some(index) = dialog.selected
                        {
                            let mut copy = dialog.configs[index].clone();
                            copy.name = free_name(&dialog.configs, &copy.name);
                            copy.temporary = false;
                            dialog.configs.push(copy);
                            dialog.selected = Some(dialog.configs.len() - 1);
                        }
                    });
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0_f32, t.border))
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(228.0, 300.0));
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if dialog.configs.is_empty() {
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(tr!("no_configurations"))
                                            .color(t.text_secondary),
                                    );
                                }
                                for (i, config) in dialog.configs.iter().enumerate() {
                                    let (rect, response) = ui::row(ui, dialog.selected == Some(i));
                                    let color = if config.temporary {
                                        t.text_secondary
                                    } else {
                                        t.text
                                    };
                                    ui::paint_icon(
                                        ui,
                                        "run",
                                        egui::Rect::from_center_size(
                                            egui::pos2(rect.left() + 18.0, rect.center().y),
                                            egui::vec2(14.0, 14.0),
                                        ),
                                    );
                                    ui::paint_text(
                                        ui,
                                        rect,
                                        rect.left() + 32.0,
                                        &config.name,
                                        color,
                                        egui::FontId::proportional(13.0),
                                    );
                                    if response.clicked() {
                                        dialog.selected = Some(i);
                                    }
                                }
                            });
                        });
                });
                ui.add_space(12.0);
                // The selected one.
                ui.vertical(|ui| {
                    let Some(config) = dialog.selected.and_then(|i| dialog.configs.get_mut(i))
                    else {
                        ui.label(
                            egui::RichText::new(tr!("choose_or_add_configuration"))
                                .color(t.text_secondary),
                        );
                        return;
                    };
                    egui::Grid::new("run_configuration_fields")
                        .num_columns(2)
                        .spacing([10.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(tr!("name_label"));
                            ui.add(
                                egui::TextEdit::singleline(&mut config.name).desired_width(380.0),
                            );
                            ui.end_row();
                            ui.label(tr!("command_label"));
                            ui.add(
                                egui::TextEdit::singleline(&mut config.command)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(380.0),
                            );
                            ui.end_row();
                            ui.label(tr!("working_directory"));
                            ui.add(
                                egui::TextEdit::singleline(&mut config.dir)
                                    .hint_text(tr!("project_root_hint"))
                                    .desired_width(380.0),
                            );
                            ui.end_row();
                        });
                    if config.temporary {
                        ui.add_space(6.0);
                        let mut keep = false;
                        if ui::checkbox(ui, &mut keep, tr!("keep_temporary_configuration"))
                            .changed()
                        {
                            config.temporary = false;
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(tr!("run_configuration_help"))
                            .small()
                            .color(t.text_secondary),
                    );
                    ui.add_space(6.0);
                    if ui::button(ui, tr!("run")).clicked() {
                        run = Some(config.clone());
                    }
                });
            });
            if let Some(error) = &dialog.error {
                ui.colored_label(t.error, error);
            }
            let (ok, cancel) = ui::dialog_buttons(ui, tr!("ok"));
            apply = ok;
            close = cancel;
        });
        if apply || run.is_some() {
            match check(&dialog.configs) {
                Ok(()) => {
                    let selected = dialog
                        .selected
                        .and_then(|i| dialog.configs.get(i))
                        .map(|c| c.name.clone());
                    let configs = self.run_configs();
                    configs.configurations = dialog.configs.clone();
                    if selected.is_some() {
                        configs.selected = selected;
                    } else if configs.selected().is_none() {
                        configs.selected = None;
                    }
                    self.save_run_configs();
                    if let Some(config) = run {
                        self.run_config(&config);
                        close = true;
                    }
                    close |= apply;
                }
                Err(e) => dialog.error = Some(e),
            }
        }
        if !close {
            self.run_dialog = Some(dialog);
        }
    }
}

/// A name none of `configs` has.
fn free_name(configs: &[RunConfig], name: &str) -> String {
    RunConfigs {
        configurations: configs.to_vec(),
        selected: None,
    }
    .free_name(name)
}

/// Every configuration has a name of its own and a command.
pub fn check(configs: &[RunConfig]) -> Result<(), String> {
    for (i, config) in configs.iter().enumerate() {
        let name = config.name.trim();
        if name.is_empty() {
            return Err(tr!("configuration_needs_name").into());
        }
        if config.command.trim().is_empty() {
            return Err(trf!("configuration_needs_command", name = name));
        }
        if configs[..i].iter().any(|c| c.name.trim() == name) {
            return Err(trf!("configuration_name_taken", name = name));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../test/app/run_configs/tests.rs"]
mod tests;
