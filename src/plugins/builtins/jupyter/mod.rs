//! Jupyter built-in plugin: `.ipynb` files open as notebooks (`notebook`) and run in a kernel of
//! the Python of the project (`kernel`), the one the Runtimes plugin chose.

pub mod kernel;
pub mod notebook;

use crate::plugins::builtin::{BuiltinPlugin, Command, EditorLayout, Host};
use crate::plugins::builtins::runtimes;
use eframe::egui;
use notebook::Request;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How often the kernels of the closed notebooks are stopped.
const CHECK_EVERY: Duration = Duration::from_secs(3);
/// The packages a notebook needs in the Python of the project.
const JUPYTER: [&str; 2] = ["ipykernel", "jupyter_client"];

#[derive(Default)]
pub struct JupyterPlugin {
    root: PathBuf,
    checked: Option<Instant>,
    layouts: HashMap<PathBuf, EditorLayout>,
    views: HashMap<PathBuf, notebook::View>,
    requests: Vec<Request>,
}

impl JupyterPlugin {
    /// The Python commands run with: the one of the project, else the one of the system.
    fn python(&self) -> String {
        runtimes::install::python(&self.root).map_or_else(
            || if cfg!(windows) { "python" } else { "python3" }.to_string(),
            |p| p.to_string_lossy().into_owned(),
        )
    }

    fn run(&self, host: &mut dyn Host, name: String, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let command = crate::plugins::command_in(&self.root, &self.root, &self.python(), &args);
        host.run_configuration(name, command);
    }

    fn install_jupyter(&self, host: &mut dyn Host) {
        let mut args = vec!["-m", "pip", "install"];
        args.extend(JUPYTER);
        self.run(host, "pip install ipykernel".into(), &args);
    }

    fn open_jupyterlab(&self, host: &mut dyn Host, path: Option<&Path>) {
        let mut args = vec!["-m".to_string(), "jupyter".into(), "lab".into()];
        if let Some(path) = path {
            args.push(path.to_string_lossy().into_owned());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(host, "jupyter lab".into(), &args);
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
}

impl BuiltinPlugin for JupyterPlugin {
    fn file_closed(&mut self, _ctx: &egui::Context, path: &Path) {
        self.views.remove(path);
    }
    fn id(&self) -> &'static str {
        "jupyter"
    }
    fn name(&self) -> &'static str {
        "Jupyter"
    }
    fn description(&self) -> &'static str {
        tr!("jupyter.description")
    }
    fn icon(&self) -> &'static str {
        "jupyter/icon"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("jupyter")
    }

    fn update(&mut self, _ctx: &egui::Context, host: &mut dyn Host) {
        if host.root() != self.root {
            *self = Self {
                root: host.root().to_path_buf(),
                ..Self::default()
            };
        }
        if self.checked.is_none_or(|at| at.elapsed() > CHECK_EVERY) {
            self.checked = Some(Instant::now());
            // The kernels of the notebooks that were closed.
            let open: HashSet<PathBuf> = host
                .open_documents()
                .into_iter()
                .filter_map(|d| d.path)
                .collect();
            self.views.retain(|path, _| open.contains(path));
        }
        for request in std::mem::take(&mut self.requests) {
            match request {
                Request::InstallJupyter => self.install_jupyter(host),
                Request::OpenJupyterLab(path) => self.open_jupyterlab(host, Some(&path)),
                Request::ShowRuntimes => host.show_tool_window(&runtimes::TOOL_WINDOWS[0]),
            }
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("jupyter.new_notebook", tr!("jupyter.command_new_notebook")),
            (
                "jupyter.install_jupyter",
                tr!("jupyter.command_install_jupyter"),
            ),
            ("jupyter.jupyterlab", tr!("jupyter.command_jupyterlab")),
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
            "jupyter.new_notebook" => self.new_notebook(host),
            "jupyter.install_jupyter" => self.install_jupyter(host),
            "jupyter.jupyterlab" => {
                let active = host.active_file().filter(|p| notebook::is_notebook(p));
                self.open_jupyterlab(host, active.as_deref());
            }
            _ => {}
        }
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
#[path = "../../../../test/plugins/builtins/jupyter/mod/tests.rs"]
mod tests;
