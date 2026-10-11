//! Node.js built-in plugin: the `package.json` files of the project, their scripts and their
//! package manager (npm, pnpm, Yarn or Bun), as IntelliJ's npm tool window shows them. A
//! script runs in a terminal of its own, from the folder of its `package.json`.

use crate::i18n::Text;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use eframe::egui::{self, FontId, RichText, Sense, vec2};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "node.scripts",
    title: Text("node.title"),
    icon: "node/icon",
    dock: Dock::Right,
    shortcut: "",
}];

/// How often the `package.json` of the root is looked at for a change.
const CHECK_EVERY: Duration = Duration::from_secs(3);
/// Folders that never hold a package of the project.
const SKIPPED: [&str; 9] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    "vendor",
    ".next",
    ".nuxt",
];
const MAX_DEPTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Manager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl Manager {
    pub fn name(self) -> &'static str {
        match self {
            Manager::Npm => "npm",
            Manager::Pnpm => "pnpm",
            Manager::Yarn => "yarn",
            Manager::Bun => "bun",
        }
    }

    /// The program to start. On Windows the `.cmd` shims, as the `.ps1` ones may be refused by
    /// the execution policy of PowerShell.
    pub fn program(self) -> String {
        match self {
            Manager::Bun => "bun".into(),
            other if cfg!(windows) => format!("{}.cmd", other.name()),
            other => other.name().into(),
        }
    }

    pub fn run(self, script: &str) -> Vec<String> {
        vec!["run".into(), script.into()]
    }

    fn from_field(value: &str) -> Option<Manager> {
        let name = value.split('@').next().unwrap_or(value);
        match name {
            "npm" => Some(Manager::Npm),
            "pnpm" => Some(Manager::Pnpm),
            "yarn" => Some(Manager::Yarn),
            "bun" => Some(Manager::Bun),
            _ => None,
        }
    }

    /// The manager of a package: its `packageManager` field, else the lockfile of its folder or
    /// of a folder above it in the project (a workspace has one lockfile at its root), else npm.
    pub fn detect(dir: &Path, root: &Path, package: &Value) -> Manager {
        if let Some(found) = package["packageManager"]
            .as_str()
            .and_then(Manager::from_field)
        {
            return found;
        }
        let locks = [
            ("bun.lockb", Manager::Bun),
            ("bun.lock", Manager::Bun),
            ("pnpm-lock.yaml", Manager::Pnpm),
            ("yarn.lock", Manager::Yarn),
            ("package-lock.json", Manager::Npm),
            ("npm-shrinkwrap.json", Manager::Npm),
        ];
        for folder in dir.ancestors() {
            if let Some((_, manager)) = locks.iter().find(|(file, _)| folder.join(file).is_file()) {
                return *manager;
            }
            if folder == root || !folder.starts_with(root) {
                break;
            }
        }
        Manager::Npm
    }
}

/// A `package.json` of the project.
#[derive(Clone, Debug)]
pub struct Package {
    pub dir: PathBuf,
    /// Its path from the root of the project, for the list.
    pub rel: String,
    pub name: String,
    pub manager: Manager,
    /// (name, command), in the order of the file.
    pub scripts: Vec<(String, String)>,
    /// It declares dependencies and they are not there.
    pub missing_modules: bool,
}

fn read_package(dir: &Path, root: &Path) -> Option<Package> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let scripts = value["scripts"]
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(name, command)| Some((name.clone(), command.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let declares = ["dependencies", "devDependencies"]
        .iter()
        .any(|key| value[*key].as_object().is_some_and(|m| !m.is_empty()));
    // The modules may be hoisted to the root of a workspace.
    let installed = dir
        .ancestors()
        .take_while(|d| d.starts_with(root))
        .any(|d| d.join("node_modules").is_dir());
    let rel = dir
        .strip_prefix(root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map_or_else(
            || "package.json".to_string(),
            |r| format!("{}/package.json", r.to_string_lossy().replace('\\', "/")),
        );
    Some(Package {
        name: value["name"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| {
                dir.file_name()
                    .map_or_else(|| "package".into(), |n| n.to_string_lossy().into_owned())
            }),
        manager: Manager::detect(dir, root, &value),
        dir: dir.to_path_buf(),
        rel,
        scripts,
        missing_modules: declares && !installed,
    })
}

/// The packages of a project, the root one first.
pub fn scan(root: &Path) -> Vec<Package> {
    fn walk(dir: &Path, root: &Path, depth: usize, out: &mut Vec<Package>) {
        if let Some(package) = read_package(dir, root) {
            out.push(package);
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !name.starts_with('.') && !SKIPPED.contains(&name)
            })
            .collect();
        dirs.sort();
        for sub in dirs {
            walk(&sub, root, depth - 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, root, MAX_DEPTH, &mut out);
    out
}

/// The line of a script in its `package.json`, to open the file there.
fn script_line(dir: &Path, script: &str) -> usize {
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
        return 0;
    };
    let start = text.find("\"scripts\"").unwrap_or(0);
    let needle = format!("\"{script}\"");
    text[start..]
        .find(&needle)
        .map_or(0, |at| text[..start + at].matches('\n').count())
}

#[derive(Default)]
pub struct NodePlugin {
    root: PathBuf,
    packages: Vec<Package>,
    scanning: Option<JoinHandle<Vec<Package>>>,
    rescan: bool,
    checked: Option<Instant>,
    stamp: Option<SystemTime>,
    /// The packages whose missing modules were already pointed out.
    offered: HashSet<PathBuf>,
    collapsed: HashSet<PathBuf>,
    selected: Option<(PathBuf, String)>,
    node_version: Option<JoinHandle<Option<String>>>,
    node: Option<Option<String>>,
}

impl NodePlugin {
    fn start_scan(&mut self) {
        if self.scanning.is_some() {
            self.rescan = true;
            return;
        }
        let root = self.root.clone();
        self.scanning = Some(thread::spawn(move || scan(&root)));
    }

    fn root_stamp(&self) -> Option<SystemTime> {
        std::fs::metadata(self.root.join("package.json"))
            .and_then(|m| m.modified())
            .ok()
    }

    fn run(&self, host: &mut dyn Host, package: &Package, args: Vec<String>) {
        let command =
            crate::plugins::command_in(&package.dir, &self.root, &package.manager.program(), &args);
        let name = format!("{} {}", package.manager.program(), args.join(" "));
        host.run_configuration(name, command);
    }

    fn install(&self, host: &mut dyn Host, package: &Package) {
        self.run(host, package, vec!["install".into()]);
    }

    /// The package the toolbar acts on: the selected one, else the root one.
    fn current(&self) -> Option<&Package> {
        self.selected
            .as_ref()
            .and_then(|(dir, _)| self.packages.iter().find(|p| &p.dir == dir))
            .or_else(|| self.packages.first())
    }
}

impl BuiltinPlugin for NodePlugin {
    fn id(&self) -> &'static str {
        "node"
    }
    fn name(&self) -> &'static str {
        "Node.js"
    }
    fn description(&self) -> &'static str {
        tr!("node.description")
    }
    fn icon(&self) -> &'static str {
        "node/icon"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }
    /// Only in a project that has one.
    fn applies(&self) -> bool {
        !self.packages.is_empty()
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if host.root() != self.root {
            *self = Self {
                root: host.root().to_path_buf(),
                ..Self::default()
            };
            self.stamp = self.root_stamp();
            self.start_scan();
        }
        if self.checked.is_none_or(|at| at.elapsed() > CHECK_EVERY) {
            self.checked = Some(Instant::now());
            let stamp = self.root_stamp();
            if stamp != self.stamp {
                self.stamp = stamp;
                self.start_scan();
            }
        }
        if self.scanning.as_ref().is_some_and(|h| h.is_finished())
            && let Some(handle) = self.scanning.take()
        {
            self.packages = handle.join().unwrap_or_default();
            if std::mem::take(&mut self.rescan) {
                self.start_scan();
            }
            // The dependencies a package declares and does not have: said once, as IntelliJ
            // offers `npm install` on a package.json.
            for package in &self.packages {
                if package.missing_modules && self.offered.insert(package.dir.clone()) {
                    host.balloon(
                        "Node.js",
                        trf!(
                            "node.modules_missing",
                            package = package.rel,
                            manager = package.manager.name()
                        ),
                        false,
                    );
                }
            }
            ctx.request_repaint();
        } else if self.scanning.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(handle) = self.node_version.take_if(|h| h.is_finished()) {
            self.node = Some(handle.join().ok().flatten());
        }
    }

    fn file_saved(&mut self, path: &Path) {
        if path.file_name().is_some_and(|n| n == "package.json") {
            self.start_scan();
        }
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: "node.install",
                label: tr!("node.command_install").into(),
            },
            Command {
                id: "node.scripts",
                label: tr!("node.command_scripts").into(),
            },
        ]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "node.install" => match self.current().cloned() {
                Some(package) => self.install(host, &package),
                None => host.notify(tr!("node.no_package").into()),
            },
            "node.scripts" => host.show_tool_window(&TOOL_WINDOWS[0]),
            _ => {}
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        if self.node.is_none() && self.node_version.is_none() {
            self.node_version = Some(thread::spawn(|| {
                let mut command = std::process::Command::new("node");
                command.arg("--version");
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x0800_0000);
                }
                let output = command.output().ok().filter(|o| o.status.success())?;
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }));
        }
        let mut run: Option<(Package, Vec<String>)> = None;
        let mut open: Option<(PathBuf, usize)> = None;
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("node.refresh")).clicked() {
                self.start_scan();
            }
            let current = self.current().cloned();
            ui.add_enabled_ui(current.is_some(), |ui| {
                if ui::icon_button(ui, "add", tr!("node.install_dependencies")).clicked()
                    && let Some(package) = &current
                {
                    run = Some((package.clone(), vec!["install".into()]));
                }
                let selected = self.selected.as_ref().map(|(_, s)| s.clone());
                if ui::icon_button(ui, "run", tr!("node.run_selected")).clicked()
                    && let (Some(package), Some(script)) = (&current, selected)
                    && !script.is_empty()
                {
                    run = Some((package.clone(), package.manager.run(&script)));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let version = match &self.node {
                    Some(Some(version)) => format!("Node {version}"),
                    Some(None) => tr!("node.node_not_found").to_string(),
                    None => String::new(),
                };
                ui.label(RichText::new(version).small().color(t.text_secondary));
            });
        });
        ui.add_space(4.0);
        if self.packages.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                let text = if self.scanning.is_some() {
                    tr!("node.scanning")
                } else {
                    tr!("node.no_package")
                };
                ui.label(RichText::new(text).color(t.text_secondary));
            });
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for package in &self.packages {
                    let open_folder = !self.collapsed.contains(&package.dir);
                    let (rect, response) = ui::row(ui, false);
                    ui::paint_chevron(
                        ui,
                        egui::pos2(rect.left() + 14.0, rect.center().y),
                        open_folder,
                    );
                    let end = ui::paint_text(
                        ui,
                        rect,
                        rect.left() + 26.0,
                        &package.rel,
                        t.text,
                        ui::semibold(12.5),
                    );
                    let end = ui::paint_badge(
                        ui,
                        ui.painter(),
                        end + 8.0,
                        rect.center().y,
                        package.manager.name(),
                        t.link,
                    );
                    if package.missing_modules {
                        ui::paint_badge(
                            ui,
                            ui.painter(),
                            end + 6.0,
                            rect.center().y,
                            tr!("node.not_installed"),
                            t.warning,
                        );
                    }
                    let response = response.on_hover_text(&package.name);
                    if response.clicked() {
                        if !self.collapsed.remove(&package.dir) {
                            self.collapsed.insert(package.dir.clone());
                        }
                        self.selected = Some((package.dir.clone(), String::new()));
                    }
                    response.context_menu(|ui| {
                        ui::menu_begin(ui);
                        if ui::menu_item(
                            ui,
                            Some("add"),
                            tr!("node.install_dependencies"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            run = Some((package.clone(), vec!["install".into()]));
                            ui.close();
                        }
                        if ui::menu_item(
                            ui,
                            Some("update"),
                            tr!("node.update_dependencies"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            let args = match package.manager {
                                Manager::Yarn => vec!["upgrade".into()],
                                _ => vec!["update".into()],
                            };
                            run = Some((package.clone(), args));
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("search"), tr!("node.outdated"), None, false)
                            .clicked()
                        {
                            run = Some((package.clone(), vec!["outdated".into()]));
                            ui.close();
                        }
                        if ui::menu_item(
                            ui,
                            Some("openNewTab"),
                            tr!("node.open_package_json"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            open = Some((package.dir.join("package.json"), 0));
                            ui.close();
                        }
                    });
                    if !open_folder {
                        continue;
                    }
                    if package.scripts.is_empty() {
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                        ui::paint_text(
                            ui,
                            rect,
                            rect.left() + 44.0,
                            tr!("node.no_scripts"),
                            t.text_secondary,
                            FontId::proportional(12.0),
                        );
                        continue;
                    }
                    for (name, command) in &package.scripts {
                        let selected = self
                            .selected
                            .as_ref()
                            .is_some_and(|(dir, s)| dir == &package.dir && s == name);
                        let (rect, response) = ui::row(ui, selected);
                        ui::paint_icon(
                            ui,
                            "run",
                            egui::Rect::from_center_size(
                                egui::pos2(rect.left() + 34.0, rect.center().y),
                                vec2(14.0, 14.0),
                            ),
                        );
                        let end = ui::paint_text(
                            ui,
                            rect,
                            rect.left() + 48.0,
                            name,
                            t.text,
                            FontId::proportional(13.0),
                        );
                        ui::paint_text(
                            ui,
                            rect,
                            end + 10.0,
                            command,
                            t.text_secondary,
                            FontId::proportional(12.0),
                        );
                        let response = response.on_hover_text(command);
                        if response.clicked() {
                            self.selected = Some((package.dir.clone(), name.clone()));
                        }
                        // A double click runs the script, as in IntelliJ.
                        if response.double_clicked() {
                            run = Some((package.clone(), package.manager.run(name)));
                        }
                        response.context_menu(|ui| {
                            ui::menu_begin(ui);
                            if ui::menu_item(
                                ui,
                                Some("run"),
                                &trf!("node.run_script", script = name),
                                None,
                                false,
                            )
                            .clicked()
                            {
                                run = Some((package.clone(), package.manager.run(name)));
                                ui.close();
                            }
                            if ui::menu_item(
                                ui,
                                Some("edit"),
                                tr!("node.jump_to_source"),
                                Some("F4"),
                                false,
                            )
                            .clicked()
                            {
                                open = Some((
                                    package.dir.join("package.json"),
                                    script_line(&package.dir, name),
                                ));
                                ui.close();
                            }
                        });
                    }
                }
            });
        if let Some((package, args)) = run {
            self.run(host, &package, args);
        }
        if let Some((path, line)) = open {
            host.open_at(path, line, 0, 0);
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/node/mod/tests.rs"]
mod tests;
