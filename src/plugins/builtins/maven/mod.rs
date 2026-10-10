//! Maven built-in plugin: the `pom.xml` files of the project, as IntelliJ's Maven tool window
//! shows them. Each project lists its lifecycle phases, its plugins and their goals, its
//! profiles and its dependencies; a phase or a goal runs in a terminal of its own, with the
//! project's wrapper (`mvnw`) when it has one.

use crate::i18n::Text;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use eframe::egui::{self, FontId, RichText, vec2};
use quick_xml::{Reader, events::Event};
use std::{
    collections::{BTreeSet, HashSet},
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "maven.projects",
    title: Text("maven.title"),
    icon: "maven/icon",
    dock: Dock::Right,
    shortcut: "",
}];

const CHECK_EVERY: Duration = Duration::from_secs(3);
const SKIPPED: [&str; 6] = ["target", "node_modules", "build", "out", "dist", "src"];
const MAX_DEPTH: usize = 4;

/// The phases of the default lifecycle IntelliJ lists, with `clean` and `site`.
pub const LIFECYCLE: [&str; 9] = [
    "clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy",
];

/// The plugins every project has, as IntelliJ lists them under Plugins.
const DEFAULT_PLUGINS: [&str; 7] = [
    "maven-clean-plugin",
    "maven-resources-plugin",
    "maven-compiler-plugin",
    "maven-surefire-plugin",
    "maven-jar-plugin",
    "maven-install-plugin",
    "maven-deploy-plugin",
];

/// The goals of the plugins that are known without reading their descriptor.
fn goals(artifact: &str) -> &'static [&'static str] {
    match artifact {
        "maven-clean-plugin" => &["clean"],
        "maven-resources-plugin" => &["resources", "testResources"],
        "maven-compiler-plugin" => &["compile", "testCompile"],
        "maven-surefire-plugin" => &["test"],
        "maven-failsafe-plugin" => &["integration-test", "verify"],
        "maven-jar-plugin" => &["jar", "test-jar"],
        "maven-war-plugin" => &["war", "exploded"],
        "maven-install-plugin" => &["install"],
        "maven-deploy-plugin" => &["deploy"],
        "maven-dependency-plugin" => &["tree", "analyze", "copy-dependencies", "resolve"],
        "maven-shade-plugin" => &["shade"],
        "maven-javadoc-plugin" => &["javadoc", "jar"],
        "maven-source-plugin" => &["jar"],
        "maven-site-plugin" => &["site"],
        "spring-boot-maven-plugin" => &["run", "build-image", "repackage", "start", "stop"],
        "exec-maven-plugin" => &["java", "exec"],
        "quarkus-maven-plugin" => &["dev", "build", "test"],
        "jetty-maven-plugin" => &["run"],
        "versions-maven-plugin" => &["display-dependency-updates", "display-plugin-updates"],
        "jacoco-maven-plugin" => &["prepare-agent", "report"],
        "maven-checkstyle-plugin" => &["check"],
        "spotless-maven-plugin" => &["apply", "check"],
        _ => &[],
    }
}

/// The prefix of a plugin's goals: `maven-compiler-plugin` is `compiler`, `spring-boot-maven-plugin`
/// is `spring-boot`.
pub fn prefix(artifact: &str) -> &str {
    artifact
        .strip_prefix("maven-")
        .and_then(|a| a.strip_suffix("-plugin"))
        .or_else(|| artifact.strip_suffix("-maven-plugin"))
        .unwrap_or(artifact)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dependency {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub scope: String,
}

#[derive(Clone, Debug, Default)]
pub struct Project {
    pub dir: PathBuf,
    /// The `pom.xml` from the root of the project, for `-f` and the list.
    pub rel: String,
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub name: String,
    pub packaging: String,
    pub modules: Vec<String>,
    /// The artifact ids of the plugins, declared or by default.
    pub plugins: Vec<String>,
    pub profiles: Vec<String>,
    pub dependencies: Vec<Dependency>,
}

/// What a `pom.xml` says, read element by element.
pub fn read_pom(text: &str) -> Option<Project> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut path: Vec<String> = Vec::new();
    let mut project = Project::default();
    let mut parent_group = String::new();
    let mut parent_version = String::new();
    let mut plugin = String::new();
    let mut dependency = Dependency::default();
    let mut seen_project = false;
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                seen_project |= path.is_empty() && name == "project";
                path.push(name);
            }
            Event::End(_) => {
                let at: Vec<&str> = path.iter().map(String::as_str).collect();
                match at.as_slice() {
                    ["project", "build", "plugins", "plugin"] => {
                        if !plugin.is_empty() && !project.plugins.contains(&plugin) {
                            project.plugins.push(std::mem::take(&mut plugin));
                        }
                    }
                    ["project", "dependencies", "dependency"] => {
                        if dependency.scope.is_empty() {
                            dependency.scope = "compile".into();
                        }
                        project.dependencies.push(std::mem::take(&mut dependency));
                    }
                    _ => {}
                }
                path.pop();
            }
            Event::Text(t) => {
                let value = t.unescape().ok()?.trim().to_string();
                let at: Vec<&str> = path.iter().map(String::as_str).collect();
                match at.as_slice() {
                    ["project", "groupId"] => project.group = value,
                    ["project", "artifactId"] => project.artifact = value,
                    ["project", "version"] => project.version = value,
                    ["project", "name"] => project.name = value,
                    ["project", "packaging"] => project.packaging = value,
                    ["project", "parent", "groupId"] => parent_group = value,
                    ["project", "parent", "version"] => parent_version = value,
                    ["project", "modules", "module"] => project.modules.push(value),
                    ["project", "build", "plugins", "plugin", "artifactId"] => plugin = value,
                    ["project", "profiles", "profile", "id"] => project.profiles.push(value),
                    ["project", "dependencies", "dependency", "groupId"] => {
                        dependency.group = value
                    }
                    ["project", "dependencies", "dependency", "artifactId"] => {
                        dependency.artifact = value
                    }
                    ["project", "dependencies", "dependency", "version"] => {
                        dependency.version = value
                    }
                    ["project", "dependencies", "dependency", "scope"] => dependency.scope = value,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_project || project.artifact.is_empty() {
        return None;
    }
    // A module takes the group and the version of its parent when it has none.
    if project.group.is_empty() {
        project.group = parent_group;
    }
    if project.version.is_empty() {
        project.version = parent_version;
    }
    if project.packaging.is_empty() {
        project.packaging = "jar".into();
    }
    for default in DEFAULT_PLUGINS {
        if !project.plugins.iter().any(|p| p == default) {
            project.plugins.push(default.to_string());
        }
    }
    Some(project)
}

/// The Maven projects of a folder, the root one first.
pub fn scan(root: &Path) -> Vec<Project> {
    fn walk(dir: &Path, root: &Path, depth: usize, out: &mut Vec<Project>) {
        let pom = dir.join("pom.xml");
        if let Some(mut project) = std::fs::read_to_string(&pom)
            .ok()
            .and_then(|t| read_pom(&t))
        {
            project.dir = dir.to_path_buf();
            project.rel = pom
                .strip_prefix(root)
                .unwrap_or(&pom)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(project);
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

/// The Maven to run: the wrapper of the project (`mvnw`), else the `mvn` of the PATH.
pub fn program(dir: &Path, root: &Path) -> String {
    let wrapper = if cfg!(windows) { "mvnw.cmd" } else { "mvnw" };
    dir.ancestors()
        .take_while(|d| d.starts_with(root))
        .map(|d| d.join(wrapper))
        .find(|p| p.is_file())
        .map_or_else(|| "mvn".to_string(), |p| p.to_string_lossy().into_owned())
}

/// The options of the toolbar, as IntelliJ has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub offline: bool,
    pub skip_tests: bool,
    pub profiles: BTreeSet<String>,
}

/// The arguments of a run: the `pom.xml` of the project, the options, then the goals.
pub fn arguments(project: &Project, options: &Options, goals: &[String]) -> Vec<String> {
    let mut args = vec!["-f".to_string(), project.rel.clone()];
    if options.offline {
        args.push("-o".into());
    }
    if options.skip_tests {
        args.push("-DskipTests".into());
    }
    let profiles: Vec<&String> = options
        .profiles
        .iter()
        .filter(|p| project.profiles.contains(p))
        .collect();
    if !profiles.is_empty() {
        args.push(format!(
            "-P{}",
            profiles
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    args.extend(goals.iter().cloned());
    args
}

/// Whether a node of the tree is open until the user says otherwise: the projects, their
/// lifecycle and profiles are; the plugins and dependencies, long lists, are not.
fn open_by_default(key: &str) -> bool {
    let parts: Vec<&str> = key.split('#').collect();
    matches!(parts.as_slice(), [_] | [_, "lifecycle" | "profiles"])
}

#[derive(Default)]
pub struct MavenPlugin {
    root: PathBuf,
    projects: Vec<Project>,
    scanning: Option<JoinHandle<Vec<Project>>>,
    rescan: bool,
    checked: Option<Instant>,
    stamp: Option<SystemTime>,
    options: Options,
    /// Keys of the nodes the user opened or closed, against `open_by_default`.
    toggled: HashSet<String>,
    selected: Option<String>,
    /// The goal typed in the toolbar (Execute Maven Goal).
    goal: String,
    goal_open: bool,
}

impl MavenPlugin {
    fn start_scan(&mut self) {
        if self.scanning.is_some() {
            self.rescan = true;
            return;
        }
        let root = self.root.clone();
        self.scanning = Some(thread::spawn(move || scan(&root)));
    }

    fn root_stamp(&self) -> Option<SystemTime> {
        std::fs::metadata(self.root.join("pom.xml"))
            .and_then(|m| m.modified())
            .ok()
    }

    fn run(&self, host: &mut dyn Host, project: &Project, goals: &[String]) {
        let args = arguments(project, &self.options, goals);
        let command = crate::plugins::command_in(
            &self.root,
            &self.root,
            &program(&project.dir, &self.root),
            &args,
        );
        let name = format!("{} [{}]", project.name, goals.join(" "));
        host.run_configuration(name, command);
    }

    /// A node of the tree: its chevron and label. Returns (response, open).
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        depth: usize,
        icon: Option<&str>,
        label: &str,
        detail: &str,
        branch: bool,
    ) -> (egui::Response, bool) {
        let t = ui::theme(ui.ctx());
        let open = branch && (open_by_default(key) != self.toggled.contains(key));
        let selected = self.selected.as_deref() == Some(key);
        let (rect, response) = ui::row(ui, selected);
        let x = rect.left() + 8.0 + depth as f32 * 16.0;
        if branch {
            ui::paint_chevron(ui, egui::pos2(x + 6.0, rect.center().y), open);
        }
        let mut text_x = x + 16.0;
        if let Some(icon) = icon {
            ui::paint_icon(
                ui,
                icon,
                egui::Rect::from_center_size(
                    egui::pos2(text_x + 7.0, rect.center().y),
                    vec2(14.0, 14.0),
                ),
            );
            text_x += 20.0;
        }
        let font = if depth == 0 {
            ui::semibold(12.5)
        } else {
            FontId::proportional(13.0)
        };
        let end = ui::paint_text(ui, rect, text_x, label, t.text, font);
        if !detail.is_empty() {
            ui::paint_text(
                ui,
                rect,
                end + 8.0,
                detail,
                t.text_secondary,
                FontId::proportional(12.0),
            );
        }
        if response.clicked() {
            self.selected = Some(key.to_string());
            if branch && !self.toggled.remove(key) {
                self.toggled.insert(key.to_string());
            }
        }
        (response, open)
    }
}

impl BuiltinPlugin for MavenPlugin {
    fn id(&self) -> &'static str {
        "maven"
    }
    fn name(&self) -> &'static str {
        "Maven"
    }
    fn description(&self) -> &'static str {
        tr!("maven.description")
    }
    fn icon(&self) -> &'static str {
        "maven/icon"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }
    /// Only in a project that has one.
    fn applies(&self) -> bool {
        !self.projects.is_empty()
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
        if let Some(handle) = self.scanning.take_if(|h| h.is_finished()) {
            self.projects = handle.join().unwrap_or_default();
            if std::mem::take(&mut self.rescan) {
                self.start_scan();
            }
            ctx.request_repaint();
        } else if self.scanning.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn file_saved(&mut self, path: &Path) {
        if path.file_name().is_some_and(|n| n == "pom.xml") {
            self.start_scan();
        }
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: "maven.reload",
                label: tr!("maven.command_reload").into(),
            },
            Command {
                id: "maven.window",
                label: tr!("maven.command_window").into(),
            },
        ]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "maven.reload" => self.start_scan(),
            "maven.window" => host.show_tool_window(&TOOL_WINDOWS[0]),
            _ => {}
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let mut run: Option<(Project, Vec<String>)> = None;
        let mut open_pom: Option<PathBuf> = None;
        let current = self
            .selected
            .as_ref()
            .and_then(|key| key.split('#').next())
            .and_then(|rel| self.projects.iter().find(|p| p.rel == rel))
            .or_else(|| self.projects.first())
            .cloned();
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("maven.reload")).clicked() {
                self.start_scan();
            }
            ui.add_enabled_ui(current.is_some(), |ui| {
                if ui::icon_toggle(ui, "run", self.goal_open, tr!("maven.execute_goal")).clicked() {
                    self.goal_open = !self.goal_open;
                }
            });
            if ui::icon_toggle(ui, "locked", self.options.offline, tr!("maven.offline")).clicked() {
                self.options.offline = !self.options.offline;
            }
            if ui::icon_toggle(ui, "hide", self.options.skip_tests, tr!("maven.skip_tests"))
                .clicked()
            {
                self.options.skip_tests = !self.options.skip_tests;
            }
            if ui::icon_button(ui, "collapseAll", tr!("collapse_all")).clicked() {
                self.toggled.clear();
                for project in &self.projects {
                    self.toggled.insert(project.rel.clone());
                }
            }
        });
        if self.goal_open
            && let Some(project) = &current
        {
            let field = ui::search_field(
                ui,
                &mut self.goal,
                tr!("maven.goal_hint"),
                ui.available_width(),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let goals: Vec<String> = self.goal.split_whitespace().map(str::to_string).collect();
                if !goals.is_empty() {
                    run = Some((project.clone(), goals));
                    self.goal_open = false;
                }
            }
        }
        ui.add_space(4.0);
        if self.projects.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                let text = if self.scanning.is_some() {
                    tr!("maven.scanning")
                } else {
                    tr!("maven.no_project")
                };
                ui.label(RichText::new(text).color(t.text_secondary));
            });
            return;
        }
        let projects = self.projects.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for project in &projects {
                    let key = project.rel.clone();
                    let detail = if project.rel == "pom.xml" {
                        project.version.clone()
                    } else {
                        format!("{} · {}", project.version, project.rel)
                    };
                    let (response, open_project) = self.node(
                        ui,
                        &key,
                        0,
                        Some("maven/icon"),
                        &project.artifact,
                        &detail,
                        true,
                    );
                    response.on_hover_text(format!(
                        "{}:{}:{}",
                        project.group, project.artifact, project.version
                    ));
                    if !open_project {
                        continue;
                    }
                    // Lifecycle.
                    let lifecycle = format!("{key}#lifecycle");
                    let (_, open) =
                        self.node(ui, &lifecycle, 1, None, tr!("maven.lifecycle"), "", true);
                    if open {
                        for phase in LIFECYCLE {
                            let leaf = format!("{lifecycle}#{phase}");
                            let (response, _) =
                                self.node(ui, &leaf, 2, Some("run"), phase, "", false);
                            if response.double_clicked() {
                                run = Some((project.clone(), vec![phase.to_string()]));
                            }
                            response.context_menu(|ui| {
                                ui::menu_begin(ui);
                                if ui::menu_item(
                                    ui,
                                    Some("run"),
                                    &trf!("maven.run_goal", goal = phase),
                                    None,
                                    false,
                                )
                                .clicked()
                                {
                                    run = Some((project.clone(), vec![phase.to_string()]));
                                    ui.close();
                                }
                            });
                        }
                    }
                    // Plugins and their goals.
                    let plugins = format!("{key}#plugins");
                    let (_, open) =
                        self.node(ui, &plugins, 1, None, tr!("maven.plugins"), "", true);
                    if open {
                        for artifact in &project.plugins {
                            let known = goals(artifact);
                            let branch = format!("{plugins}#{artifact}");
                            let (_, open) = self.node(
                                ui,
                                &branch,
                                2,
                                None,
                                prefix(artifact),
                                artifact,
                                !known.is_empty(),
                            );
                            if !open {
                                continue;
                            }
                            for goal in known {
                                let invocation = format!("{}:{goal}", prefix(artifact));
                                let leaf = format!("{branch}#{goal}");
                                let (response, _) =
                                    self.node(ui, &leaf, 3, Some("run"), &invocation, "", false);
                                if response.double_clicked() {
                                    run = Some((project.clone(), vec![invocation.clone()]));
                                }
                                response.context_menu(|ui| {
                                    ui::menu_begin(ui);
                                    if ui::menu_item(
                                        ui,
                                        Some("run"),
                                        &trf!("maven.run_goal", goal = invocation),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        run = Some((project.clone(), vec![invocation.clone()]));
                                        ui.close();
                                    }
                                });
                            }
                        }
                    }
                    // Profiles, ticked for the next runs.
                    if !project.profiles.is_empty() {
                        let profiles = format!("{key}#profiles");
                        let (_, open) =
                            self.node(ui, &profiles, 1, None, tr!("maven.profiles"), "", true);
                        if open {
                            for profile in &project.profiles {
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0 + 2.0 * 16.0 + 16.0);
                                    let mut on = self.options.profiles.contains(profile);
                                    if ui::checkbox(ui, &mut on, profile).changed() {
                                        if on {
                                            self.options.profiles.insert(profile.clone());
                                        } else {
                                            self.options.profiles.remove(profile);
                                        }
                                    }
                                });
                            }
                        }
                    }
                    // Dependencies, as the pom declares them.
                    let dependencies = format!("{key}#dependencies");
                    let count = project.dependencies.len().to_string();
                    let (_, open) = self.node(
                        ui,
                        &dependencies,
                        1,
                        None,
                        tr!("maven.dependencies"),
                        &count,
                        !project.dependencies.is_empty(),
                    );
                    if open {
                        for dependency in &project.dependencies {
                            let label = format!("{}:{}", dependency.group, dependency.artifact);
                            let detail = if dependency.version.is_empty() {
                                dependency.scope.clone()
                            } else {
                                format!("{} · {}", dependency.version, dependency.scope)
                            };
                            let leaf = format!("{dependencies}#{label}");
                            let (response, _) =
                                self.node(ui, &leaf, 2, None, &label, &detail, false);
                            if response.double_clicked() {
                                open_pom = Some(project.dir.join("pom.xml"));
                            }
                        }
                    }
                }
            });
        if let Some((project, goals)) = run {
            self.run(host, &project, &goals);
        }
        if let Some(path) = open_pom {
            host.open_file(path);
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/maven/mod/tests.rs"]
mod tests;
