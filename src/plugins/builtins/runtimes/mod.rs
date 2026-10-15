//! Runtimes built-in plugin: a Python, JDKs and Node.js installed in the project itself
//! (`install`), put first on the PATH of its terminals, and Jupyter notebooks run with the
//! Python of the project (`notebook`, `kernel`).

pub mod install;
pub mod kernel;
pub mod notebook;

use crate::i18n::Text;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, EditorLayout, Host, ToolWindow};
use crate::ui;
use eframe::egui::{self, RichText};
use install::{Kind, Offer, Step};
use notebook::Request;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub use install::environment;

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "runtimes.window",
    title: Text("runtimes.title"),
    icon: "runtimes/icon",
    dock: Dock::Right,
    shortcut: "",
}];

/// How often the installed runtimes and the pin files are looked at again.
const CHECK_EVERY: Duration = Duration::from_secs(3);
/// The packages a notebook needs in the Python of the project.
const JUPYTER: [&str; 2] = ["ipykernel", "jupyter_client"];

enum Catalog {
    Loading(JoinHandle<Result<Vec<Offer>, String>>),
    Ready(Vec<Offer>),
    Failed(String),
}

/// What an installation installs: a version of the catalog, or the newest one a pin file
/// asks for.
#[derive(Clone)]
enum Target {
    Offer(Offer),
    Wanted(String),
}

struct Job {
    kind: Kind,
    label: String,
    progress: Arc<Mutex<(Step, u64, u64)>>,
    handle: JoinHandle<Result<String, String>>,
    _task: crate::tasks::Task,
}

/// What the window shows of a kind of runtime.
#[derive(Default, Clone)]
struct State {
    installed: Vec<String>,
    active: Option<String>,
    pinned: Option<(&'static str, String)>,
}

impl State {
    fn read(root: &Path, kind: Kind) -> Self {
        Self {
            installed: install::installed(root, kind),
            active: install::active(root, kind),
            pinned: install::pinned(root, kind),
        }
    }

    /// The pinned version, when no installed version gives it.
    fn missing(&self) -> Option<&(&'static str, String)> {
        self.pinned.as_ref().filter(|(_, wanted)| {
            !self
                .installed
                .iter()
                .any(|v| install::satisfies(wanted, v, false))
        })
    }
}

#[derive(Default)]
pub struct RuntimesPlugin {
    root: PathBuf,
    states: HashMap<Kind, State>,
    venv: Option<PathBuf>,
    checked: Option<Instant>,
    catalogs: HashMap<Kind, Catalog>,
    chosen: HashMap<Kind, usize>,
    job: Option<Job>,
    /// The pin files already pointed out.
    offered: HashSet<Kind>,
    layouts: HashMap<PathBuf, EditorLayout>,
    views: HashMap<PathBuf, notebook::View>,
    requests: Vec<Request>,
}

impl RuntimesPlugin {
    fn refresh(&mut self) {
        self.checked = Some(Instant::now());
        for kind in Kind::ALL {
            self.states.insert(kind, State::read(&self.root, kind));
        }
        self.venv = install::venv(&self.root).map(|(dir, _)| dir);
    }

    fn state(&self, kind: Kind) -> State {
        self.states.get(&kind).cloned().unwrap_or_default()
    }

    fn load_catalog(&mut self, kind: Kind) {
        if matches!(
            self.catalogs.get(&kind),
            Some(Catalog::Loading(_) | Catalog::Ready(_))
        ) {
            return;
        }
        self.catalogs.insert(
            kind,
            Catalog::Loading(thread::spawn(move || install::catalog(kind))),
        );
    }

    fn start_install(&mut self, kind: Kind, target: Target, host: &mut dyn Host) {
        if self.job.is_some() {
            host.notify(tr!("runtimes.busy").into());
            return;
        }
        let label = match &target {
            Target::Offer(offer) => format!("{} {}", kind.name(), offer.version),
            Target::Wanted(wanted) => format!("{} {wanted}", kind.name()),
        };
        let progress = Arc::new(Mutex::new((Step::Resolving, 0, 0)));
        let shared = progress.clone();
        let root = self.root.clone();
        let handle = thread::spawn(move || {
            let report = |step, done, total| {
                *shared.lock().unwrap_or_else(|e| e.into_inner()) = (step, done, total);
            };
            let offer = match target {
                Target::Offer(offer) => offer,
                Target::Wanted(wanted) => install::catalog(kind)?
                    .into_iter()
                    .find(|o| install::satisfies(&wanted, &o.version, o.lts))
                    .ok_or_else(|| trf!("runtimes.no_such_version", version = wanted))?,
            };
            install::install(&root, kind, &offer, &report)
        });
        self.job = Some(Job {
            kind,
            _task: crate::tasks::start(trf!("runtimes.installing", runtime = label)),
            label,
            progress,
            handle,
        });
    }

    /// The Python commands run with: the one of the project, else the one of the system.
    fn python(&self) -> String {
        install::python(&self.root).map_or_else(
            || if cfg!(windows) { "python" } else { "python3" }.to_string(),
            |p| p.to_string_lossy().into_owned(),
        )
    }

    fn run(&self, host: &mut dyn Host, name: String, program: &str, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let command = crate::plugins::command_in(&self.root, &self.root, program, &args);
        host.run_configuration(name, command);
    }

    fn create_venv(&self, host: &mut dyn Host) {
        let python = install::program(&self.root, Kind::Python).map_or_else(
            || if cfg!(windows) { "python" } else { "python3" }.to_string(),
            |p| p.to_string_lossy().into_owned(),
        );
        self.run(
            host,
            "python -m venv .venv".into(),
            &python,
            &["-m", "venv", ".venv"],
        );
    }

    fn install_jupyter(&self, host: &mut dyn Host) {
        let mut args = vec!["-m", "pip", "install"];
        args.extend(JUPYTER);
        self.run(host, "pip install ipykernel".into(), &self.python(), &args);
    }

    fn open_jupyterlab(&self, host: &mut dyn Host, path: Option<&Path>) {
        let mut args = vec!["-m".to_string(), "jupyter".into(), "lab".into()];
        if let Some(path) = path {
            args.push(path.to_string_lossy().into_owned());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(host, "jupyter lab".into(), &self.python(), &args);
    }

    fn new_notebook(&self, host: &mut dyn Host) {
        let path = (0..)
            .map(|n| match n {
                0 => self.root.join("Untitled.ipynb"),
                n => self.root.join(format!("Untitled{n}.ipynb")),
            })
            .find(|p| !p.exists())
            .expect("a free name");
        match std::fs::write(&path, notebook::to_text(&notebook::empty())) {
            Ok(()) => {
                host.reload_files();
                host.open_file(path);
            }
            Err(e) => host.notify(format!("{}{e}", tr!("error_prefix"))),
        }
    }

    fn section(&mut self, ui: &mut egui::Ui, kind: Kind, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let state = self.state(kind);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(kind.name()).font(ui::semibold(13.5)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let used = match &state.active {
                    Some(version) => trf!("runtimes.uses_project", version = version),
                    None => tr!("runtimes.uses_system").to_string(),
                };
                ui.label(RichText::new(used).small().color(t.text_secondary));
            });
        });
        // The versions to choose from: the one of the system, and those of the project.
        if ui::radio(ui, state.active.is_none(), tr!("runtimes.system_version")).clicked() {
            install::set_active(&self.root, kind, None);
            self.refresh();
        }
        let mut remove = None;
        for version in &state.installed {
            ui.horizontal(|ui| {
                let active = state.active.as_deref() == Some(version.as_str());
                if ui::radio(ui, active, version).clicked() {
                    install::set_active(&self.root, kind, Some(version));
                    self.refresh();
                }
                if ui::icon_button(ui, "delete", tr!("runtimes.uninstall")).clicked() {
                    remove = Some(version.clone());
                }
            });
        }
        if let Some(version) = remove {
            if let Err(e) = install::uninstall(&self.root, kind, &version) {
                host.notify(format!("{}{e}", tr!("error_prefix")));
            }
            self.refresh();
        }
        if let Some((file, wanted)) = state.missing() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(trf!(
                        "runtimes.pinned_missing",
                        file = file,
                        version = wanted
                    ))
                    .small()
                    .color(t.warning),
                );
                if self.job.is_none()
                    && ui::button(ui, &trf!("runtimes.install_version", version = wanted)).clicked()
                {
                    self.start_install(kind, Target::Wanted(wanted.clone()), host);
                }
            });
        }
        // Installing another version.
        if let Some(job) = self.job.as_ref().filter(|j| j.kind == kind) {
            let (step, done, total) = *job.progress.lock().unwrap_or_else(|e| e.into_inner());
            let (fraction, text) = match step {
                Step::Resolving => (0.0, tr!("runtimes.resolving").to_string()),
                Step::Downloading if total > 0 => (
                    done as f32 / total as f32,
                    format!("{} — {} / {} MB", job.label, done >> 20, total >> 20),
                ),
                Step::Downloading => (0.0, format!("{} — {} MB", job.label, done >> 20)),
                Step::Extracting => (1.0, tr!("runtimes.extracting").to_string()),
            };
            ui.add(egui::ProgressBar::new(fraction).text(text).animate(true));
            ui.ctx().request_repaint_after(Duration::from_millis(200));
            return;
        }
        let mut chosen_install = None;
        ui.horizontal(|ui| match self.catalogs.get(&kind) {
            Some(Catalog::Ready(offers)) if !offers.is_empty() => {
                let index = self
                    .chosen
                    .get(&kind)
                    .copied()
                    .unwrap_or(0)
                    .min(offers.len() - 1);
                let label = |o: &Offer| match o.lts {
                    true => format!("{} LTS", o.version),
                    false => o.version.clone(),
                };
                let mut selected = index;
                egui::ComboBox::from_id_salt(("runtimes.offers", kind.id()))
                    .selected_text(label(&offers[index]))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for (i, offer) in offers.iter().enumerate() {
                            ui.selectable_value(&mut selected, i, label(offer));
                        }
                    });
                self.chosen.insert(kind, selected);
                if ui::primary_button(ui, tr!("runtimes.install")).clicked() {
                    chosen_install = Some(offers[selected].clone());
                }
            }
            Some(Catalog::Ready(_)) => {
                ui.label(
                    RichText::new(tr!("runtimes.unsupported_system"))
                        .small()
                        .color(t.text_secondary),
                );
            }
            Some(Catalog::Failed(why)) => {
                ui.label(
                    RichText::new(tr!("runtimes.catalog_failed"))
                        .small()
                        .color(t.error),
                )
                .on_hover_text(why);
                if ui::icon_button(ui, "refresh", tr!("runtimes.retry")).clicked() {
                    self.catalogs.remove(&kind);
                }
            }
            Some(Catalog::Loading(_)) => {
                ui.spinner();
                ui.label(RichText::new(tr!("runtimes.loading_versions")).small());
            }
            None => {
                if ui::button(ui, tr!("runtimes.install_other")).clicked() {
                    self.load_catalog(kind);
                }
            }
        });
        if let Some(offer) = chosen_install {
            self.start_install(kind, Target::Offer(offer), host);
        }
    }

    fn python_extras(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        ui.add_space(4.0);
        match &self.venv {
            Some(dir) => {
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
                ui.label(
                    RichText::new(trf!(
                        "runtimes.venv_present",
                        name = name.unwrap_or_default()
                    ))
                    .small()
                    .color(t.text_secondary),
                );
            }
            None => {
                if ui::button(ui, tr!("runtimes.create_venv")).clicked() {
                    self.create_venv(host);
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui::button(ui, tr!("runtimes.install_jupyter")).clicked() {
                self.install_jupyter(host);
            }
            if ui::button(ui, tr!("runtimes.new_notebook")).clicked() {
                self.new_notebook(host);
            }
        });
    }
}

impl BuiltinPlugin for RuntimesPlugin {
    fn id(&self) -> &'static str {
        "runtimes"
    }
    fn name(&self) -> &'static str {
        "Runtimes & Jupyter"
    }
    fn description(&self) -> &'static str {
        tr!("runtimes.description")
    }
    fn icon(&self) -> &'static str {
        "runtimes/icon"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("runtimes")
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if host.root() != self.root {
            *self = Self {
                root: host.root().to_path_buf(),
                ..Self::default()
            };
            self.refresh();
        }
        if self.checked.is_none_or(|at| at.elapsed() > CHECK_EVERY) {
            self.refresh();
            // A version a pin file asks for and the project does not have: said once.
            for kind in Kind::ALL {
                if let Some((file, wanted)) = self.state(kind).missing()
                    && self.offered.insert(kind)
                {
                    host.balloon(
                        tr!("runtimes.title"),
                        trf!(
                            "runtimes.pinned_balloon",
                            runtime = kind.name(),
                            version = wanted,
                            file = file
                        ),
                        false,
                    );
                }
            }
            // The kernels of the notebooks that were closed.
            let open: HashSet<PathBuf> = host
                .open_documents()
                .into_iter()
                .filter_map(|d| d.path)
                .collect();
            self.views.retain(|path, _| open.contains(path));
        }
        for kind in Kind::ALL {
            if let Some(Catalog::Loading(handle)) = self.catalogs.get(&kind)
                && handle.is_finished()
                && let Some(Catalog::Loading(handle)) = self.catalogs.remove(&kind)
            {
                let catalog = match handle.join() {
                    Ok(Ok(offers)) => Catalog::Ready(offers),
                    Ok(Err(why)) => Catalog::Failed(why),
                    Err(_) => Catalog::Failed(String::new()),
                };
                self.catalogs.insert(kind, catalog);
                ctx.request_repaint();
            } else if matches!(self.catalogs.get(&kind), Some(Catalog::Loading(_))) {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if let Some(job) = self.job.take_if(|j| j.handle.is_finished()) {
            match job.handle.join() {
                Ok(Ok(version)) => {
                    install::set_active(&self.root, job.kind, Some(&version));
                    host.balloon(
                        tr!("runtimes.title"),
                        trf!(
                            "runtimes.installed",
                            runtime = job.kind.name(),
                            version = version
                        ),
                        false,
                    );
                }
                Ok(Err(why)) => host.balloon(tr!("runtimes.title"), why, true),
                Err(_) => host.balloon(tr!("runtimes.title"), job.label, true),
            }
            self.refresh();
            ctx.request_repaint();
        }
        for request in std::mem::take(&mut self.requests) {
            match request {
                Request::InstallJupyter => self.install_jupyter(host),
                Request::OpenJupyterLab(path) => self.open_jupyterlab(host, Some(&path)),
                Request::ShowRuntimes => host.show_tool_window(&TOOL_WINDOWS[0]),
            }
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("runtimes.window", tr!("runtimes.command_window")),
            (
                "runtimes.new_notebook",
                tr!("runtimes.command_new_notebook"),
            ),
            (
                "runtimes.install_jupyter",
                tr!("runtimes.command_install_jupyter"),
            ),
            ("runtimes.jupyterlab", tr!("runtimes.command_jupyterlab")),
            ("runtimes.venv", tr!("runtimes.command_venv")),
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
            "runtimes.window" => host.show_tool_window(&TOOL_WINDOWS[0]),
            "runtimes.new_notebook" => self.new_notebook(host),
            "runtimes.install_jupyter" => self.install_jupyter(host),
            "runtimes.jupyterlab" => {
                let active = host.active_file().filter(|p| notebook::is_notebook(p));
                self.open_jupyterlab(host, active.as_deref());
            }
            "runtimes.venv" => self.create_venv(host),
            _ => {}
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("runtimes.refresh")).clicked() {
                self.catalogs.clear();
                self.refresh();
            }
            ui.label(
                RichText::new(tr!("runtimes.terminal_hint"))
                    .small()
                    .color(t.text_secondary),
            );
        });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for kind in Kind::ALL {
                    ui.push_id(kind.id(), |ui| {
                        self.section(ui, kind, host);
                        if kind == Kind::Python {
                            self.python_extras(ui, host);
                        }
                    });
                    ui.add_space(6.0);
                    ui.separator();
                }
            });
    }

    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        notebook::is_notebook(path).then(|| {
            self.layouts
                .get(path)
                .copied()
                .unwrap_or(EditorLayout::Preview)
        })
    }

    fn set_editor_layout(&mut self, path: &Path, layout: EditorLayout) {
        self.layouts.insert(path.to_path_buf(), layout);
    }

    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, _scroll: Option<f32>) {
        let root = self.root.clone();
        self.views.entry(path.to_path_buf()).or_default().show(
            ui,
            &root,
            path,
            text,
            &mut self.requests,
        );
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/runtimes/mod/tests.rs"]
mod tests;
