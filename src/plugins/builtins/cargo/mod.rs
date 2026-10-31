//! Cargo built-in plugin: the `Cargo.toml` files of the project, packages and workspaces, and
//! the targets Cargo finds in them (binaries, examples, tests, benchmarks), as IntelliJ's Cargo
//! tool window shows them. A command runs in a terminal of its own, from the folder of its
//! `Cargo.toml`.

use crate::i18n::Text;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use eframe::egui::{self, FontId, RichText, Sense, vec2};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "cargo.window",
    title: Text("cargo.title"),
    icon: "cargo/icon",
    dock: Dock::Right,
    shortcut: "",
}];

/// How often the `Cargo.toml` of the root is looked at for a change.
const CHECK_EVERY: Duration = Duration::from_secs(3);
/// Folders that never hold a package of the project.
const SKIPPED: [&str; 3] = ["target", "node_modules", "vendor"];
const MAX_DEPTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bin,
    Example,
    Test,
    Bench,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Bin, Kind::Example, Kind::Test, Kind::Bench];

    /// The name of its tables in `Cargo.toml` (`[[bin]]`), and of its flag (`--bin`).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bin => "bin",
            Kind::Example => "example",
            Kind::Test => "test",
            Kind::Bench => "bench",
        }
    }

    /// The folder Cargo finds these targets in, and the key that turns that off (`autobins`).
    fn folder(self) -> (&'static str, &'static str) {
        match self {
            Kind::Bin => ("src/bin", "autobins"),
            Kind::Example => ("examples", "autoexamples"),
            Kind::Test => ("tests", "autotests"),
            Kind::Bench => ("benches", "autobenches"),
        }
    }
}

/// A target of a package: what `cargo run`, `cargo test` or `cargo bench` can pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub kind: Kind,
    pub name: String,
    /// Its source file, when it is there.
    pub path: Option<PathBuf>,
}

impl Target {
    /// The arguments of cargo that run it.
    pub fn args(&self) -> Vec<String> {
        let command = match self.kind {
            Kind::Bin | Kind::Example => "run",
            Kind::Test => "test",
            Kind::Bench => "bench",
        };
        vec![
            command.into(),
            format!("--{}", self.kind.name()),
            self.name.clone(),
        ]
    }
}

/// A `Cargo.toml` of the project.
#[derive(Clone, Debug)]
pub struct Package {
    pub dir: PathBuf,
    /// Its path from the root of the project, for the list.
    pub rel: String,
    pub name: String,
    pub version: Option<String>,
    /// It has a `[workspace]` table.
    pub workspace: bool,
    pub targets: Vec<Target>,
}

/// The targets of a package: those its tables declare, then those Cargo finds in their folders
/// (`src/main.rs`, `src/bin/*.rs`, `examples/*.rs`, `tests/*.rs`, `benches/*.rs`, or a folder
/// there with a `main.rs`).
fn targets(dir: &Path, doc: &toml_edit::DocumentMut, package: &str) -> Vec<Target> {
    fn add(out: &mut Vec<Target>, target: Target) {
        if !out
            .iter()
            .any(|t| t.kind == target.kind && t.name == target.name)
        {
            out.push(target);
        }
    }
    let mut out = Vec::new();
    for kind in Kind::ALL {
        let (folder, auto) = kind.folder();
        if let Some(tables) = doc.get(kind.name()).and_then(|t| t.as_array_of_tables()) {
            for table in tables.iter() {
                let Some(name) = table.get("name").and_then(|n| n.as_str()) else {
                    continue;
                };
                let path = table.get("path").and_then(|p| p.as_str()).map_or_else(
                    || dir.join(folder).join(format!("{name}.rs")),
                    |p| dir.join(p),
                );
                add(
                    &mut out,
                    Target {
                        kind,
                        name: name.to_string(),
                        path: path.is_file().then_some(path),
                    },
                );
            }
        }
        let automatic = doc
            .get("package")
            .and_then(|p| p.get(auto))
            .and_then(|a| a.as_bool())
            .unwrap_or(true);
        if !automatic {
            continue;
        }
        if kind == Kind::Bin && dir.join("src/main.rs").is_file() {
            add(
                &mut out,
                Target {
                    kind,
                    name: package.to_string(),
                    path: Some(dir.join("src/main.rs")),
                },
            );
        }
        let Ok(entries) = std::fs::read_dir(dir.join(folder)) else {
            continue;
        };
        let mut found: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    let main = path.join("main.rs");
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    main.is_file().then_some((name, main))
                } else if path.extension().is_some_and(|e| e == "rs") {
                    Some((path.file_stem()?.to_string_lossy().into_owned(), path))
                } else {
                    None
                }
            })
            .collect();
        found.sort();
        for (name, path) in found {
            add(
                &mut out,
                Target {
                    kind,
                    name,
                    path: Some(path),
                },
            );
        }
    }
    out
}

fn read_package(dir: &Path, root: &Path) -> Option<Package> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let package = doc.get("package");
    let name = package
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            dir.file_name()
                .map_or_else(|| "workspace".into(), |n| n.to_string_lossy().into_owned())
        });
    let rel = dir
        .strip_prefix(root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map_or_else(
            || "Cargo.toml".to_string(),
            |r| format!("{}/Cargo.toml", r.to_string_lossy().replace('\\', "/")),
        );
    Some(Package {
        targets: match package {
            Some(_) => targets(dir, &doc, &name),
            None => Vec::new(),
        },
        version: package
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        workspace: doc.contains_key("workspace"),
        name,
        dir: dir.to_path_buf(),
        rel,
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

/// The commands of a package, for its menu: icon, label and arguments of cargo.
fn package_commands() -> [(&'static str, &'static str, &'static [&'static str]); 8] {
    [
        ("cargo/icons/build", tr!("cargo.build"), &["build"]),
        ("inspectionsOK", tr!("cargo.check"), &["check"]),
        ("run", tr!("cargo.test"), &["test"]),
        (
            "problems",
            tr!("cargo.clippy"),
            &["clippy", "--all-targets"],
        ),
        ("edit", tr!("cargo.fmt"), &["fmt"]),
        ("info", tr!("cargo.doc"), &["doc", "--open"]),
        ("refresh", tr!("cargo.update"), &["update"]),
        ("delete", tr!("cargo.clean"), &["clean"]),
    ]
}

#[derive(Default)]
pub struct CargoPlugin {
    root: PathBuf,
    packages: Vec<Package>,
    scanning: Option<JoinHandle<Vec<Package>>>,
    rescan: bool,
    checked: Option<Instant>,
    stamp: Option<SystemTime>,
    collapsed: HashSet<PathBuf>,
    /// The selected package, and the selected target in it.
    selected: Option<(PathBuf, Option<Target>)>,
    cargo_version: Option<JoinHandle<Option<String>>>,
    cargo: Option<Option<String>>,
}

impl CargoPlugin {
    fn start_scan(&mut self) {
        if self.scanning.is_some() {
            self.rescan = true;
            return;
        }
        let root = self.root.clone();
        self.scanning = Some(thread::spawn(move || scan(&root)));
    }

    fn root_stamp(&self) -> Option<SystemTime> {
        std::fs::metadata(self.root.join("Cargo.toml"))
            .and_then(|m| m.modified())
            .ok()
    }

    fn run(&self, host: &mut dyn Host, package: &Package, args: Vec<String>) {
        let command = crate::plugins::command_in(&package.dir, &self.root, "cargo", &args);
        host.run_configuration(format!("cargo {}", args.join(" ")), command);
    }

    /// The package the toolbar acts on: the selected one, else the root one.
    fn current(&self) -> Option<&Package> {
        self.selected
            .as_ref()
            .and_then(|(dir, _)| self.packages.iter().find(|p| &p.dir == dir))
            .or_else(|| self.packages.first())
    }

    fn run_current(&self, host: &mut dyn Host, command: &str) {
        match self.current().cloned() {
            Some(package) => self.run(host, &package, vec![command.into()]),
            None => host.notify(tr!("cargo.no_crate").into()),
        }
    }
}

impl BuiltinPlugin for CargoPlugin {
    fn id(&self) -> &'static str {
        "cargo"
    }
    fn name(&self) -> &'static str {
        "Cargo"
    }
    fn description(&self) -> &'static str {
        tr!("cargo.description")
    }
    fn icon(&self) -> &'static str {
        "cargo/icon"
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
            ctx.request_repaint();
        } else if self.scanning.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(handle) = self.cargo_version.take_if(|h| h.is_finished()) {
            self.cargo = Some(handle.join().ok().flatten());
        }
    }

    fn file_saved(&mut self, path: &Path) {
        // A manifest, or a file of a folder Cargo finds targets in.
        let folder = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str());
        if path.file_name().is_some_and(|n| n == "Cargo.toml")
            || matches!(folder, Some("bin" | "examples" | "tests" | "benches"))
        {
            self.start_scan();
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("cargo.window", tr!("cargo.command_window")),
            ("cargo.build", tr!("cargo.command_build")),
            ("cargo.check", tr!("cargo.command_check")),
            ("cargo.test", tr!("cargo.command_test")),
        ]
        .into_iter()
        .map(|(id, label)| Command {
            id,
            label: label.into(),
        })
        .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "cargo.window" => host.show_tool_window(&TOOL_WINDOWS[0]),
            "cargo.build" => self.run_current(host, "build"),
            "cargo.check" => self.run_current(host, "check"),
            "cargo.test" => self.run_current(host, "test"),
            _ => {}
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        if self.cargo.is_none() && self.cargo_version.is_none() {
            self.cargo_version = Some(thread::spawn(|| {
                let mut command = std::process::Command::new("cargo");
                command.arg("--version");
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x0800_0000);
                }
                let output = command.output().ok().filter(|o| o.status.success())?;
                // `cargo 1.84.0 (66221abde 2024-11-19)`: the name and the version.
                let text = String::from_utf8_lossy(&output.stdout);
                Some(
                    text.split_whitespace()
                        .take(2)
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }));
        }
        let mut run: Option<(Package, Vec<String>)> = None;
        let mut open: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("cargo.refresh")).clicked() {
                self.start_scan();
            }
            let current = self.current().cloned();
            ui.add_enabled_ui(current.is_some(), |ui| {
                if ui::icon_button(ui, "cargo/icons/build", tr!("cargo.build")).clicked()
                    && let Some(package) = &current
                {
                    run = Some((package.clone(), vec!["build".into()]));
                }
                let target = self.selected.as_ref().and_then(|(_, t)| t.clone());
                if ui::icon_button(ui, "run", tr!("cargo.run_selected")).clicked()
                    && let (Some(package), Some(target)) = (&current, target)
                {
                    run = Some((package.clone(), target.args()));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let version = match &self.cargo {
                    Some(Some(version)) => version.clone(),
                    Some(None) => tr!("cargo.cargo_not_found").to_string(),
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
                    tr!("cargo.scanning")
                } else {
                    tr!("cargo.no_crate")
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
                    let selected = self
                        .selected
                        .as_ref()
                        .is_some_and(|(dir, t)| dir == &package.dir && t.is_none());
                    let (rect, response) = ui::row(ui, selected);
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
                    if package.workspace {
                        ui::paint_badge(
                            ui,
                            ui.painter(),
                            end + 8.0,
                            rect.center().y,
                            tr!("cargo.workspace"),
                            t.link,
                        );
                    }
                    let hover = match &package.version {
                        Some(version) => format!("{} {version}", package.name),
                        None => package.name.clone(),
                    };
                    let response = response.on_hover_text(hover);
                    if response.clicked() {
                        if !self.collapsed.remove(&package.dir) {
                            self.collapsed.insert(package.dir.clone());
                        }
                        self.selected = Some((package.dir.clone(), None));
                    }
                    response.context_menu(|ui| {
                        ui::menu_begin(ui);
                        for (icon, label, args) in package_commands() {
                            if ui::menu_item(ui, Some(icon), label, None, false).clicked() {
                                let args = args.iter().map(|a| a.to_string()).collect();
                                run = Some((package.clone(), args));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui::menu_item(
                            ui,
                            Some("openNewTab"),
                            tr!("cargo.open_manifest"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            open = Some(package.dir.join("Cargo.toml"));
                            ui.close();
                        }
                    });
                    if !open_folder {
                        continue;
                    }
                    if package.targets.is_empty() {
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                        ui::paint_text(
                            ui,
                            rect,
                            rect.left() + 44.0,
                            tr!("cargo.no_targets"),
                            t.text_secondary,
                            FontId::proportional(12.0),
                        );
                        continue;
                    }
                    for target in &package.targets {
                        let selected = self.selected.as_ref().is_some_and(|(dir, t)| {
                            dir == &package.dir && t.as_ref() == Some(target)
                        });
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
                            &target.name,
                            t.text,
                            FontId::proportional(13.0),
                        );
                        ui::paint_text(
                            ui,
                            rect,
                            end + 10.0,
                            target.kind.name(),
                            t.text_secondary,
                            FontId::proportional(12.0),
                        );
                        let response =
                            response.on_hover_text(format!("cargo {}", target.args().join(" ")));
                        if response.clicked() {
                            self.selected = Some((package.dir.clone(), Some(target.clone())));
                        }
                        // A double click runs the target, as in IntelliJ.
                        if response.double_clicked() {
                            run = Some((package.clone(), target.args()));
                        }
                        response.context_menu(|ui| {
                            ui::menu_begin(ui);
                            if ui::menu_item(
                                ui,
                                Some("run"),
                                &trf!("cargo.run_target", target = target.name),
                                None,
                                false,
                            )
                            .clicked()
                            {
                                run = Some((package.clone(), target.args()));
                                ui.close();
                            }
                            if let Some(path) = &target.path
                                && ui::menu_item(
                                    ui,
                                    Some("edit"),
                                    tr!("cargo.jump_to_source"),
                                    Some("F4"),
                                    false,
                                )
                                .clicked()
                            {
                                open = Some(path.clone());
                                ui.close();
                            }
                        });
                    }
                }
            });
        if let Some((package, args)) = run {
            self.run(host, &package, args);
        }
        if let Some(path) = open {
            host.open_file(path);
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/cargo/mod/tests.rs"]
mod tests;
