//! Gradle built-in plugin: the projects of a Gradle build and their tasks, as IntelliJ's Gradle
//! tool window shows them. The projects come from `settings.gradle(.kts)`; the tasks are first
//! those the plugins of the build file bring, then, after a reload, the real list Gradle gives
//! (`tasks --all`). A task runs in a terminal of its own, with the wrapper (`gradlew`) when the
//! build has one.

use crate::i18n::Text;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use eframe::egui::{self, FontId, RichText, vec2};
use regex::Regex;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::LazyLock,
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "gradle.projects",
    title: Text("gradle.title"),
    icon: "gradle/icon",
    dock: Dock::Right,
    shortcut: "",
}];

const CHECK_EVERY: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub name: String,
    pub description: String,
}

/// Tasks by group, in the order Gradle names the groups.
pub type Groups = BTreeMap<String, Vec<Task>>;

#[derive(Clone, Debug)]
pub struct Project {
    pub dir: PathBuf,
    /// `:` for the root, `:app` for a subproject.
    pub path: String,
    pub name: String,
    /// The plugins the build file applies (`java`, `application`, `org.springframework.boot`…).
    pub plugins: Vec<String>,
    pub tasks: Groups,
}

static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["']([^"']+)["']"#).expect("regex"));
static ROOT_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"rootProject\.name\s*=\s*["']([^"']+)["']"#).expect("regex"));
static INCLUDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*include\b\s*\(?([^\n)]*)").expect("regex"));
static PLUGIN_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:\bid\s*\(?\s*["']([^"']+)["']|apply\s+plugin:\s*["']([^"']+)["']|\bkotlin\s*\(\s*["']([^"']+)["']\s*\))"#)
        .expect("regex")
});
/// The core plugins of Gradle, which the `plugins` block names alone (`java`, `application`).
const CORE_PLUGINS: [&str; 9] = [
    "java-library",
    "java",
    "application",
    "groovy",
    "scala",
    "war",
    "jacoco",
    "checkstyle",
    "base",
];

fn build_file(dir: &Path) -> Option<PathBuf> {
    ["build.gradle.kts", "build.gradle"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

fn settings_file(dir: &Path) -> Option<PathBuf> {
    ["settings.gradle.kts", "settings.gradle"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// The plugins a build file applies.
pub fn plugins_of(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for caps in PLUGIN_ID.captures_iter(text) {
        let id = caps
            .get(1)
            .or(caps.get(2))
            .map(|m| m.as_str().to_string())
            .or_else(|| {
                caps.get(3)
                    .map(|m| format!("org.jetbrains.kotlin.{}", m.as_str()))
            });
        if let Some(id) = id
            && !out.contains(&id)
        {
            out.push(id);
        }
    }
    // `plugins { java; application }`: a core plugin named alone, on its line or not.
    if let Some(start) = text.find("plugins") {
        let block = &text[start + "plugins".len()..];
        let end = block.find('}').unwrap_or(block.len());
        for piece in block[..end].split(['\n', ';', '{']) {
            let name = piece.trim().trim_matches('`');
            if CORE_PLUGINS.contains(&name) && !out.iter().any(|p| p == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// The tasks the plugins bring, before Gradle is asked: what IntelliJ shows for such a build.
pub fn default_tasks(plugins: &[String], root: bool) -> Groups {
    let mut groups = Groups::new();
    let mut add = |group: &str, tasks: &[(&str, &str)]| {
        let list = groups.entry(group.to_string()).or_default();
        for (name, description) in tasks {
            if !list.iter().any(|t| t.name == *name) {
                list.push(Task {
                    name: name.to_string(),
                    description: description.to_string(),
                });
            }
        }
    };
    let has = |id: &str| plugins.iter().any(|p| p == id || p.ends_with(id));
    let jvm = [
        "java",
        "java-library",
        "application",
        "groovy",
        "scala",
        "war",
    ]
    .iter()
    .any(|p| has(p))
        || plugins
            .iter()
            .any(|p| p.starts_with("org.jetbrains.kotlin") || p == "org.springframework.boot");
    if jvm {
        add(
            "build",
            &[
                ("assemble", "Assembles the outputs of this project."),
                ("build", "Assembles and tests this project."),
                ("classes", "Assembles main classes."),
                ("clean", "Deletes the build directory."),
                (
                    "jar",
                    "Assembles a jar archive containing the classes of the 'main' feature.",
                ),
                ("testClasses", "Assembles test classes."),
            ],
        );
        add(
            "verification",
            &[
                ("check", "Runs all checks."),
                ("test", "Runs the test suite."),
            ],
        );
        add(
            "documentation",
            &[(
                "javadoc",
                "Generates Javadoc API documentation for the 'main' feature.",
            )],
        );
    }
    if has("application") {
        add(
            "application",
            &[("run", "Runs this project as a JVM application")],
        );
        add(
            "distribution",
            &[
                ("distZip", "Bundles the project as a distribution."),
                (
                    "installDist",
                    "Installs the project as a distribution as-is.",
                ),
            ],
        );
    }
    if has("org.springframework.boot") {
        add(
            "application",
            &[("bootRun", "Runs this project as a Spring Boot application.")],
        );
        add(
            "build",
            &[(
                "bootJar",
                "Assembles an executable jar archive containing the main classes and their dependencies.",
            )],
        );
    }
    if has("war") {
        add(
            "build",
            &[(
                "war",
                "Generates a war archive with all the compiled classes, the web-app content and the libraries.",
            )],
        );
    }
    if root {
        add(
            "build setup",
            &[
                ("init", "Initializes a new Gradle build."),
                ("wrapper", "Generates Gradle wrapper files."),
            ],
        );
    }
    add(
        "help",
        &[
            (
                "dependencies",
                "Displays all dependencies declared in this project.",
            ),
            ("projects", "Displays the sub-projects of this project."),
            ("properties", "Displays the properties of this project."),
            ("tasks", "Displays the tasks runnable from this project."),
        ],
    );
    groups
}

/// The projects of a build: the root one, then those `settings.gradle` includes.
pub fn scan(root: &Path) -> Vec<Project> {
    let settings = settings_file(root).and_then(|p| std::fs::read_to_string(p).ok());
    if settings.is_none() && build_file(root).is_none() {
        return Vec::new();
    }
    let settings = settings.unwrap_or_default();
    let root_name = ROOT_NAME
        .captures(&settings)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| {
            root.file_name()
                .map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned())
        });
    let mut paths = vec![":".to_string()];
    for line in INCLUDE.captures_iter(&settings) {
        for quoted in QUOTED.captures_iter(&line[1]) {
            let path = format!(":{}", quoted[1].trim_start_matches(':'));
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let dir = if path == ":" {
                root.to_path_buf()
            } else {
                root.join(path.trim_start_matches(':').replace(':', "/"))
            };
            let plugins = build_file(&dir)
                .and_then(|f| std::fs::read_to_string(f).ok())
                .map(|t| plugins_of(&t))
                .unwrap_or_default();
            let name = if path == ":" {
                root_name.clone()
            } else {
                path.rsplit(':').next().unwrap_or(&path).to_string()
            };
            Project {
                tasks: default_tasks(&plugins, path == ":"),
                dir,
                path,
                name,
                plugins,
            }
        })
        .collect()
}

/// The tasks of `gradle tasks --all`, by project path then group.
pub fn parse_tasks(output: &str) -> BTreeMap<String, Groups> {
    let mut out: BTreeMap<String, Groups> = BTreeMap::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut group: Option<String> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_end();
        let underline = lines.get(i + 1).map(|l| l.trim());
        // A heading: a line underlined with dashes.
        if !line.is_empty()
            && underline.is_some_and(|u| !u.is_empty() && u.chars().all(|c| c == '-'))
        {
            let name = line.trim().trim_end_matches(" tasks").to_lowercase();
            group = (name != "rules" && !name.starts_with("tasks runnable")).then_some(name);
            i += 2;
            continue;
        }
        if line.is_empty() {
            i += 1;
            continue;
        }
        if let Some(group) = &group {
            let (name, description) = line.split_once(" - ").unwrap_or((line, ""));
            let name = name.trim();
            if !name.contains(' ') {
                let (project, task) = match name.rsplit_once(':') {
                    Some((project, task)) => {
                        (format!(":{}", project.trim_start_matches(':')), task)
                    }
                    None => (":".to_string(), name),
                };
                out.entry(project)
                    .or_default()
                    .entry(group.clone())
                    .or_default()
                    .push(Task {
                        name: task.to_string(),
                        description: description.trim().to_string(),
                    });
            }
        }
        i += 1;
    }
    out
}

/// The wrapper of the build (`gradlew`), else the `gradle` of the PATH.
pub fn program(root: &Path) -> String {
    let wrapper = root.join(if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    });
    if wrapper.is_file() {
        wrapper.to_string_lossy().into_owned()
    } else {
        "gradle".into()
    }
}

/// The path of a task to run from the root: `build` for the root project, `:app:build` for another.
pub fn task_path(project: &str, task: &str) -> String {
    if project == ":" {
        task.to_string()
    } else {
        format!("{project}:{task}")
    }
}

/// The program to start in the background: the wrapper, else a `gradle` found on the PATH.
fn background_program(root: &Path) -> Option<PathBuf> {
    let found = program(root);
    if found != "gradle" {
        return Some(PathBuf::from(found));
    }
    let names: &[&str] = if cfg!(windows) {
        &["gradle.bat", "gradle.cmd", "gradle.exe"]
    } else {
        &["gradle"]
    };
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// Asks Gradle for the tasks of the build.
fn load_tasks(root: &Path) -> Result<BTreeMap<String, Groups>, String> {
    let program = background_program(root).ok_or_else(|| tr!("gradle.not_found").to_string())?;
    let mut command = std::process::Command::new(program);
    command
        .args(["tasks", "--all", "--console=plain", "-q"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        let shown: Vec<&str> = error
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(4)
            .collect();
        return Err(shown.join(" "));
    }
    Ok(parse_tasks(&String::from_utf8_lossy(&output.stdout)))
}

/// Whether a node of the tree is open until the user says otherwise: the projects and their
/// tasks are, the groups of tasks are not.
fn open_by_default(key: &str) -> bool {
    key.split('#').count() <= 2
}

#[derive(Default)]
pub struct GradlePlugin {
    root: PathBuf,
    projects: Vec<Project>,
    stamp: Option<SystemTime>,
    checked: Option<Instant>,
    /// Gradle is asked for the tasks.
    loading: Option<JoinHandle<Result<BTreeMap<String, Groups>, String>>>,
    /// The tasks come from Gradle, not from the defaults.
    loaded: bool,
    offline: bool,
    /// Keys of the nodes the user opened or closed, against `open_by_default`.
    toggled: HashSet<String>,
    selected: Option<String>,
    task: String,
    task_open: bool,
}

impl GradlePlugin {
    fn settings_stamp(&self) -> Option<SystemTime> {
        settings_file(&self.root)
            .or_else(|| build_file(&self.root))
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
    }

    fn rescan(&mut self) {
        self.projects = scan(&self.root);
        self.loaded = false;
    }

    /// Reload, as IntelliJ's button: the projects again, then the tasks from Gradle.
    fn reload(&mut self) {
        self.rescan();
        if self.loading.is_none() && !self.projects.is_empty() {
            let root = self.root.clone();
            self.loading = Some(thread::spawn(move || load_tasks(&root)));
        }
    }

    fn run(&self, host: &mut dyn Host, tasks: Vec<String>) {
        let mut args = tasks;
        if self.offline {
            args.push("--offline".into());
        }
        let command =
            crate::plugins::command_in(&self.root, &self.root, &program(&self.root), &args);
        host.run_in_new_terminal(command);
    }

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
        let (rect, response) = ui::row(ui, self.selected.as_deref() == Some(key));
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

impl BuiltinPlugin for GradlePlugin {
    fn id(&self) -> &'static str {
        "gradle"
    }
    fn name(&self) -> &'static str {
        "Gradle"
    }
    fn description(&self) -> &'static str {
        tr!("gradle.description")
    }
    fn icon(&self) -> &'static str {
        "gradle/icon"
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
            self.stamp = self.settings_stamp();
            self.rescan();
        }
        if self.checked.is_none_or(|at| at.elapsed() > CHECK_EVERY) {
            self.checked = Some(Instant::now());
            let stamp = self.settings_stamp();
            if stamp != self.stamp {
                self.stamp = stamp;
                self.rescan();
            }
        }
        if let Some(handle) = self.loading.take_if(|h| h.is_finished()) {
            match handle.join().unwrap_or_else(|_| Err(String::new())) {
                Ok(mut tasks) => {
                    for project in &mut self.projects {
                        if let Some(groups) = tasks.remove(&project.path) {
                            project.tasks = groups;
                        }
                    }
                    self.loaded = true;
                }
                Err(e) => host.balloon("Gradle", trf!("gradle.cannot_load_tasks", e = e), true),
            }
            ctx.request_repaint();
        } else if self.loading.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn file_saved(&mut self, path: &Path) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("settings.gradle") || name.starts_with("build.gradle") {
            self.rescan();
        }
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: "gradle.reload",
                label: tr!("gradle.command_reload").into(),
            },
            Command {
                id: "gradle.window",
                label: tr!("gradle.command_window").into(),
            },
        ]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "gradle.reload" => self.reload(),
            "gradle.window" => host.show_tool_window(&TOOL_WINDOWS[0]),
            _ => {}
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let mut run: Option<Vec<String>> = None;
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("gradle.reload")).clicked() {
                self.reload();
            }
            if ui::icon_toggle(ui, "run", self.task_open, tr!("gradle.execute_task")).clicked() {
                self.task_open = !self.task_open;
            }
            if ui::icon_toggle(ui, "locked", self.offline, tr!("gradle.offline")).clicked() {
                self.offline = !self.offline;
            }
            if ui::icon_button(ui, "collapseAll", tr!("collapse_all")).clicked() {
                self.toggled.clear();
                for project in &self.projects {
                    self.toggled.insert(project.path.clone());
                }
            }
            if self.loading.is_some() {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(
                    RichText::new(tr!("gradle.loading"))
                        .small()
                        .color(t.text_secondary),
                );
            }
        });
        if self.task_open {
            let field = ui::search_field(
                ui,
                &mut self.task,
                tr!("gradle.task_hint"),
                ui.available_width(),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let tasks: Vec<String> = self.task.split_whitespace().map(str::to_string).collect();
                if !tasks.is_empty() {
                    run = Some(tasks);
                    self.task_open = false;
                }
            }
        }
        ui.add_space(4.0);
        if self.projects.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("gradle.no_project")).color(t.text_secondary));
            });
            return;
        }
        if !self.loaded && self.loading.is_none() {
            ui.add(
                egui::Label::new(
                    RichText::new(tr!("gradle.default_tasks"))
                        .small()
                        .color(t.text_secondary),
                )
                .wrap(),
            );
            ui.add_space(4.0);
        }
        let projects = self.projects.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for project in &projects {
                    let key = project.path.clone();
                    let detail = if project.path == ":" {
                        String::new()
                    } else {
                        project.path.clone()
                    };
                    let (response, open) = self.node(
                        ui,
                        &key,
                        0,
                        Some("gradle/icon"),
                        &project.name,
                        &detail,
                        true,
                    );
                    let response = if project.plugins.is_empty() {
                        response
                    } else {
                        response.on_hover_text(project.plugins.join(", "))
                    };
                    response.context_menu(|ui| {
                        ui::menu_begin(ui);
                        if ui::menu_item(
                            ui,
                            Some("showAsTree"),
                            tr!("gradle.show_dependencies"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            run = Some(vec![task_path(&project.path, "dependencies")]);
                            ui.close();
                        }
                        if let Some(file) = build_file(&project.dir)
                            && ui::menu_item(
                                ui,
                                Some("openNewTab"),
                                tr!("gradle.open_build_file"),
                                None,
                                false,
                            )
                            .clicked()
                        {
                            host.open_file(file);
                            ui.close();
                        }
                    });
                    if !open {
                        continue;
                    }
                    let tasks_key = format!("{key}#tasks");
                    let (_, open) =
                        self.node(ui, &tasks_key, 1, None, tr!("gradle.tasks"), "", true);
                    if !open {
                        continue;
                    }
                    for (group, tasks) in &project.tasks {
                        let group_key = format!("{tasks_key}#{group}");
                        let (_, open) = self.node(ui, &group_key, 2, None, group, "", true);
                        if !open {
                            continue;
                        }
                        for task in tasks {
                            let leaf = format!("{group_key}#{}", task.name);
                            let (response, _) =
                                self.node(ui, &leaf, 3, Some("run"), &task.name, "", false);
                            let response = if task.description.is_empty() {
                                response
                            } else {
                                response.on_hover_text(&task.description)
                            };
                            let path = task_path(&project.path, &task.name);
                            if response.double_clicked() {
                                run = Some(vec![path.clone()]);
                            }
                            response.context_menu(|ui| {
                                ui::menu_begin(ui);
                                if ui::menu_item(
                                    ui,
                                    Some("run"),
                                    &trf!("gradle.run_task", task = path),
                                    None,
                                    false,
                                )
                                .clicked()
                                {
                                    run = Some(vec![path.clone()]);
                                    ui.close();
                                }
                            });
                        }
                    }
                }
            });
        if let Some(tasks) = run {
            self.run(host, tasks);
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/gradle/mod/tests.rs"]
mod tests;
