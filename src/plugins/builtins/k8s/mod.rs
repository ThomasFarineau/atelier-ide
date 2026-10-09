//! Kubernetes built-in plugin: browse the resources of a cluster, read their description, YAML
//! and logs, restart and delete them, with the installed `kubectl` and its kubeconfig.

use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use anyhow::{Result, anyhow, bail};
use eframe::egui::{self, Color32, RichText};
use std::{
    process::{Command as Process, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "k8s.resources",
    title: "Kubernetes",
    icon: "k8s/icon",
    dock: Dock::Bottom,
    shortcut: "Alt+8",
}];

const REFRESH_EVERY: Duration = Duration::from_secs(5);
/// The window is considered closed when it was not drawn for this long.
const VISIBLE_FOR: Duration = Duration::from_millis(1500);
const MAX_TEXT: usize = 300_000;

/// (label, `kubectl` resource, namespaced)
const KINDS: [(&str, &str, bool); 12] = [
    ("Pods", "pods", true),
    ("Deployments", "deployments", true),
    ("StatefulSets", "statefulsets", true),
    ("DaemonSets", "daemonsets", true),
    ("Jobs", "jobs", true),
    ("CronJobs", "cronjobs", true),
    ("Services", "services", true),
    ("Ingresses", "ingresses", true),
    ("ConfigMaps", "configmaps", true),
    ("Secrets", "secrets", true),
    ("Nodes", "nodes", false),
    ("Namespaces", "namespaces", false),
];

fn kubectl(args: &[String]) -> Result<String> {
    kubectl_input(args, None)
}

fn kubectl_input(args: &[String], input: Option<String>) -> Result<String> {
    let mut cmd = Process::new("kubectl");
    cmd.args(args)
        .arg("--request-timeout=15s")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow!("kubectl est introuvable : installez-le et ajoutez-le au PATH")
        } else {
            anyhow!("Impossible de lancer kubectl : {e}")
        }
    })?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        use std::io::Write;
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("{}", err.trim());
    }
    if text.len() > MAX_TEXT {
        let mut cut = text.len() - MAX_TEXT;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        text = format!("… (début tronqué)\n{}", &text[cut..]);
    }
    Ok(text)
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// Output of `kubectl get`, split by the column positions of the header.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub fn parse_table(text: &str) -> Table {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Table::default();
    };
    let chars: Vec<char> = header.chars().collect();
    // A column starts after at least two spaces: header names may contain one ("NOMINATED NODE").
    let mut starts = vec![0];
    for i in 2..chars.len() {
        if chars[i] != ' ' && chars[i - 1] == ' ' && chars[i - 2] == ' ' {
            starts.push(i);
        }
    }
    let cut = |line: &str| -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        starts
            .iter()
            .enumerate()
            .map(|(n, &from)| {
                let to = starts.get(n + 1).copied().unwrap_or(chars.len());
                let to = to.min(chars.len());
                if from >= to {
                    String::new()
                } else {
                    chars[from..to].iter().collect::<String>().trim().to_string()
                }
            })
            .collect()
    };
    Table {
        headers: cut(header),
        rows: lines.map(cut).collect(),
    }
}

/// Age as printed by kubectl (`3d`, `5h`, `2m30s`, `1y2d`) in seconds.
fn age_seconds(age: &str) -> u64 {
    let (mut total, mut number) = (0u64, 0u64);
    for c in age.chars() {
        if let Some(d) = c.to_digit(10) {
            number = number * 10 + u64::from(d);
        } else {
            total += number
                * match c {
                    's' => 1,
                    'm' => 60,
                    'h' => 3600,
                    'd' => 86_400,
                    'y' => 31_536_000,
                    _ => 0,
                };
            number = 0;
        }
    }
    total
}

fn leading_number(text: &str) -> u64 {
    text.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Compares two cells of the column `header`: ages and restart counts numerically, `READY`
/// (`1/2`) by ready then total, the rest as text without case.
fn compare_cells(header: &str, a: &str, b: &str) -> std::cmp::Ordering {
    match header {
        "AGE" => age_seconds(a).cmp(&age_seconds(b)),
        "RESTARTS" => leading_number(a).cmp(&leading_number(b)),
        "READY" => {
            let split = |s: &str| {
                let (ready, total) = s.split_once('/').unwrap_or((s, ""));
                (leading_number(ready), leading_number(total))
            };
            split(a).cmp(&split(b))
        }
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let (mut out, mut bits, mut count) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    Some(out)
}

/// A ConfigMap or a Secret being edited key by key.
struct Editor {
    resource: &'static str,
    ns: String,
    name: String,
    doc: serde_json::Value,
    entries: Vec<(String, String)>,
    /// Binary values, kept as they are.
    binary: Vec<String>,
    new_key: String,
    error: Option<String>,
}

impl Editor {
    fn new(resource: &'static str, doc: serde_json::Value) -> Result<Self> {
        use serde_json::Value;
        let mut doc = doc;
        let secret = resource == "secrets";
        let mut entries = Vec::new();
        let mut binary = Vec::new();
        let mut kept = serde_json::Map::new();
        if let Some(data) = doc.get("data").and_then(Value::as_object) {
            for (key, value) in data {
                let text = value.as_str().unwrap_or_default();
                if !secret {
                    entries.push((key.clone(), text.to_string()));
                } else if let Some(decoded) = b64_decode(text).and_then(|b| String::from_utf8(b).ok()) {
                    entries.push((key.clone(), decoded));
                } else {
                    binary.push(key.clone());
                    kept.insert(key.clone(), value.clone());
                }
            }
        }
        if let Some(extra) = doc.get("binaryData").and_then(Value::as_object) {
            binary.extend(extra.keys().cloned());
        }
        let obj = doc.as_object_mut().ok_or_else(|| anyhow!("réponse inattendue de kubectl"))?;
        if let Some(meta) = obj.get_mut("metadata").and_then(Value::as_object_mut) {
            meta.remove("managedFields");
        }
        obj.remove("data");
        if !kept.is_empty() {
            obj.insert("data".into(), Value::Object(kept));
        }
        let meta = |k: &str| {
            obj.get("metadata")
                .and_then(|m| m.get(k))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        Ok(Self {
            resource,
            ns: meta("namespace"),
            name: meta("name"),
            doc,
            entries,
            binary,
            new_key: String::new(),
            error: None,
        })
    }

    /// The object to send back: text values in `data` (ConfigMap) or `stringData` (Secret).
    fn document(&self) -> serde_json::Value {
        use serde_json::{Map, Value};
        let mut doc = self.doc.clone();
        let values: Map<String, Value> = self
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        if let Some(obj) = doc.as_object_mut() {
            if self.resource == "secrets" {
                if !values.is_empty() {
                    obj.insert("stringData".into(), Value::Object(values));
                }
            } else {
                obj.insert("data".into(), Value::Object(values));
            }
        }
        doc
    }

    fn add_key(&mut self) {
        let key = self.new_key.trim().to_string();
        let valid = !key.is_empty()
            && key.len() <= 253
            && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !valid {
            self.error = Some("Clé invalide : lettres, chiffres, « . », « _ » et « - » seulement".into());
        } else if self.entries.iter().any(|(k, _)| *k == key) || self.binary.contains(&key) {
            self.error = Some(format!("La clé {key} existe déjà"));
        } else {
            self.entries.push((key, String::new()));
            self.new_key.clear();
            self.error = None;
        }
    }
}

struct Detail {
    title: String,
    text: String,
    error: bool,
}

struct Confirm {
    title: String,
    text: String,
    label: String,
    args: Vec<String>,
}

struct Task<T> {
    handle: JoinHandle<Result<T>>,
}

impl<T: Send + 'static> Task<T> {
    fn spawn(f: impl FnOnce() -> Result<T> + Send + 'static) -> Self {
        Self {
            handle: thread::spawn(f),
        }
    }
    fn poll(task: &mut Option<Self>) -> Option<Result<T>> {
        if task.as_ref().is_some_and(|t| t.handle.is_finished()) {
            let t = task.take()?;
            return Some(t.handle.join().unwrap_or_else(|_| Err(anyhow!("la commande a planté"))));
        }
        None
    }
}

struct Meta {
    contexts: Vec<String>,
    current: Option<String>,
}

#[derive(Default)]
pub struct K8sPlugin {
    contexts: Vec<String>,
    context: Option<String>,
    namespaces: Vec<String>,
    /// Empty: the namespace of the context.
    namespace: String,
    all_namespaces: bool,
    kind: usize,
    filter: String,
    /// Column header and ascending order.
    sort: Option<(String, bool)>,
    table: Table,
    error: Option<String>,
    loaded_for: Option<String>,
    refreshed_at: Option<Instant>,
    drawn_at: Option<Instant>,
    stale: bool,
    selected: Option<(String, String)>,
    detail: Option<Detail>,
    editor: Option<Editor>,
    editor_job: Option<(&'static str, Task<serde_json::Value>)>,
    saving: bool,
    confirm: Option<Confirm>,
    meta_job: Option<Task<Meta>>,
    namespaces_job: Option<Task<Vec<String>>>,
    list_job: Option<Task<(String, Table)>>,
    detail_job: Option<(String, Task<String>)>,
    action_job: Option<(String, Task<String>)>,
    meta_loaded: bool,
}

impl K8sPlugin {
    fn visible(&self) -> bool {
        self.drawn_at.is_some_and(|t| t.elapsed() < VISIBLE_FOR)
    }

    fn namespaced(&self) -> bool {
        KINDS[self.kind].2
    }

    /// `--context` and `--namespace` flags of the current selection.
    fn scope(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(context) = &self.context {
            args.extend(["--context".into(), context.clone()]);
        }
        if self.namespaced() {
            if self.all_namespaces {
                args.push("--all-namespaces".into());
            } else if !self.namespace.is_empty() {
                args.extend(["--namespace".into(), self.namespace.clone()]);
            }
        }
        args
    }

    fn list_key(&self) -> String {
        format!("{:?}|{}|{}|{}", self.context, self.namespace, self.all_namespaces, self.kind)
    }

    fn reload(&mut self) {
        self.stale = true;
    }

    fn load_meta(&mut self) {
        self.meta_loaded = true;
        self.meta_job = Some(Task::spawn(|| {
            let contexts = kubectl(&strings(&["config", "get-contexts", "-o", "name"]))?
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            let current = kubectl(&strings(&["config", "current-context"]))
                .ok()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty());
            Ok(Meta { contexts, current })
        }));
    }

    fn load_namespaces(&mut self) {
        let mut args = strings(&["get", "namespaces", "-o", "jsonpath={.items[*].metadata.name}"]);
        if let Some(context) = &self.context {
            args.extend(["--context".into(), context.clone()]);
        }
        self.namespaces_job = Some(Task::spawn(move || {
            Ok(kubectl(&args)?.split_whitespace().map(str::to_string).collect())
        }));
    }

    fn load_list(&mut self) {
        let key = self.list_key();
        let mut args = strings(&["get", KINDS[self.kind].1]);
        args.extend(self.scope());
        self.stale = false;
        self.refreshed_at = Some(Instant::now());
        self.list_job = Some(Task::spawn(move || Ok((key, parse_table(&kubectl(&args)?)))));
    }

    /// Name and namespace columns of a row.
    fn identity(&self, row: &[String]) -> Option<(String, String)> {
        let name_col = usize::from(self.namespaced() && self.all_namespaces);
        let name = row.get(name_col)?.clone();
        let ns = if name_col == 1 {
            row[0].clone()
        } else {
            self.namespace.clone()
        };
        Some((ns, name))
    }

    fn target_args(&self, ns: &str) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(context) = &self.context {
            args.extend(["--context".into(), context.clone()]);
        }
        if self.namespaced() && !ns.is_empty() {
            args.extend(["--namespace".into(), ns.to_string()]);
        }
        args
    }

    fn show(&mut self, title: String, mut args: Vec<String>, ns: &str) {
        args.extend(self.target_args(ns));
        self.editor = None;
        self.detail = Some(Detail {
            title: title.clone(),
            text: "Chargement…".into(),
            error: false,
        });
        self.detail_job = Some((title, Task::spawn(move || kubectl(&args))));
    }

    fn run_action(&mut self, label: String, args: Vec<String>, ns: &str) {
        self.run_action_with(label, args, None, ns);
    }

    fn run_action_with(&mut self, label: String, mut args: Vec<String>, input: Option<String>, ns: &str) {
        if self.action_job.is_some() {
            return;
        }
        args.extend(self.target_args(ns));
        self.action_job = Some((label, Task::spawn(move || kubectl_input(&args, input))));
    }

    fn can_edit(&self) -> bool {
        matches!(KINDS[self.kind].1, "configmaps" | "secrets")
    }

    fn open_editor(&mut self, ns: &str, name: &str) {
        let resource = KINDS[self.kind].1;
        let mut args = strings(&["get", resource, name, "-o", "json"]);
        args.extend(self.target_args(ns));
        self.detail = None;
        self.detail_job = None;
        self.editor = None;
        self.editor_job = Some((
            resource,
            Task::spawn(move || Ok(serde_json::from_str(&kubectl(&args)?)?)),
        ));
    }

    fn save_editor(&mut self) {
        let Some(editor) = &self.editor else {
            return;
        };
        let input = editor.document().to_string();
        let label = format!("Enregistrement de {}", editor.name);
        let ns = editor.ns.clone();
        self.saving = true;
        self.run_action_with(label, strings(&["replace", "-f", "-"]), Some(input), &ns);
        if self.action_job.is_none() {
            self.saving = false;
        }
    }

    fn describe(&mut self, ns: &str, name: &str) {
        let resource = KINDS[self.kind].1;
        self.show(format!("describe {name}"), strings(&["describe", resource, name]), ns);
    }

    fn yaml(&mut self, ns: &str, name: &str) {
        let resource = KINDS[self.kind].1;
        self.show(format!("{name}.yaml"), strings(&["get", resource, name, "-o", "yaml"]), ns);
    }

    fn logs(&mut self, ns: &str, name: &str) {
        self.show(
            format!("logs {name}"),
            strings(&["logs", name, "--all-containers", "--tail=500", "--timestamps"]),
            ns,
        );
    }

    fn can_restart(&self) -> bool {
        matches!(
            KINDS[self.kind].1,
            "pods" | "deployments" | "statefulsets" | "daemonsets"
        )
    }

    /// Asks to `rollout restart` the selected deployment, statefulset or daemonset, or to
    /// delete the selected pod so that its controller recreates it.
    fn ask_restart(&mut self) {
        let Some((_, name)) = self.selected.clone() else {
            return;
        };
        let resource = KINDS[self.kind].1;
        if resource == "pods" {
            self.confirm = Some(Confirm {
                title: "Redémarrer".into(),
                text: format!(
                    "Redémarrer le pod {name} ? Il est supprimé puis recréé par son contrôleur ; un pod sans contrôleur ne reviendra pas."
                ),
                label: format!("Redémarrage de {name}"),
                args: strings(&["delete", "pod", &name]),
            });
            return;
        }
        self.confirm = Some(Confirm {
            title: "Redémarrer".into(),
            text: format!("Redémarrer {resource}/{name} (rollout restart) ?"),
            label: format!("Redémarrage de {name}"),
            args: [strings(&["rollout", "restart"]), vec![format!("{resource}/{name}")]].concat(),
        });
    }

    fn poll(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if self.visible() && !self.meta_loaded {
            self.load_meta();
        }
        if let Some(result) = Task::poll(&mut self.meta_job) {
            match result {
                Ok(meta) => {
                    self.contexts = meta.contexts;
                    if self.context.is_none() {
                        self.context = meta.current;
                    }
                    self.load_namespaces();
                    self.reload();
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        if let Some(Ok(namespaces)) = Task::poll(&mut self.namespaces_job) {
            self.namespaces = namespaces;
        }
        if let Some(result) = Task::poll(&mut self.list_job) {
            match result {
                Ok((key, table)) => {
                    if key == self.list_key() {
                        self.table = table;
                        self.error = None;
                        self.loaded_for = Some(key);
                    }
                }
                Err(e) => {
                    self.error = Some(format!("{e:#}"));
                    self.table = Table::default();
                    self.loaded_for = Some(self.list_key());
                }
            }
        }
        if let Some((title, task)) = self.detail_job.take() {
            let mut slot = Some(task);
            match Task::poll(&mut slot) {
                Some(result) => {
                    if self.detail.as_ref().is_some_and(|d| d.title == title) {
                        let (text, error) = match result {
                            Ok(text) if text.trim().is_empty() => ("(vide)".to_string(), false),
                            Ok(text) => (text, false),
                            Err(e) => (format!("{e:#}"), true),
                        };
                        self.detail = Some(Detail { title, text, error });
                    }
                }
                None => self.detail_job = slot.map(|t| (title, t)),
            }
        }
        if let Some((resource, task)) = self.editor_job.take() {
            let mut slot = Some(task);
            match Task::poll(&mut slot) {
                Some(Ok(doc)) => match Editor::new(resource, doc) {
                    Ok(editor) => self.editor = Some(editor),
                    Err(e) => host.balloon("Édition impossible", format!("{e:#}"), true),
                },
                Some(Err(e)) => host.balloon("Édition impossible", format!("{e:#}"), true),
                None => self.editor_job = slot.map(|t| (resource, t)),
            }
        }
        if let Some((label, task)) = self.action_job.take() {
            let mut slot = Some(task);
            match Task::poll(&mut slot) {
                Some(Ok(_)) => {
                    host.balloon("Kubernetes", format!("{label} : terminé"), false);
                    if std::mem::take(&mut self.saving) {
                        self.editor = None;
                    }
                    self.reload();
                }
                Some(Err(e)) => {
                    self.saving = false;
                    host.balloon(&format!("{label} a échoué"), format!("{e:#}"), true);
                }
                None => self.action_job = slot.map(|t| (label, t)),
            }
        }
        if self.visible() {
            if self.loaded_for.as_deref() != Some(&self.list_key()) && self.list_job.is_none() {
                self.stale = true;
            }
            if self.refreshed_at.is_none_or(|t| t.elapsed() > REFRESH_EVERY) {
                self.stale = true;
            }
            if self.stale && self.list_job.is_none() && self.meta_loaded && self.meta_job.is_none() {
                self.load_list();
            }
        }
        let busy = self.list_job.is_some()
            || self.meta_job.is_some()
            || self.namespaces_job.is_some()
            || self.detail_job.is_some()
            || self.editor_job.is_some()
            || self.action_job.is_some();
        if busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.visible() {
            ctx.request_repaint_after(REFRESH_EVERY);
        }
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut changed = false;
            ui.label("Contexte");
            let current = self.context.clone().unwrap_or_else(|| "—".into());
            egui::ComboBox::from_id_salt("k8s_context")
                .selected_text(current)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for name in self.contexts.clone() {
                        let on = self.context.as_deref() == Some(&name);
                        if ui.selectable_label(on, &name).clicked() && !on {
                            self.context = Some(name);
                            self.namespace.clear();
                            self.namespaces.clear();
                            self.selected = None;
                            self.load_namespaces();
                            changed = true;
                        }
                    }
                });
            if self.namespaced() {
                ui.label("Namespace");
                let shown = if self.all_namespaces {
                    "tous".to_string()
                } else if self.namespace.is_empty() {
                    "défaut".to_string()
                } else {
                    self.namespace.clone()
                };
                egui::ComboBox::from_id_salt("k8s_namespace")
                    .selected_text(shown)
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.all_namespaces, "tous").clicked() {
                            self.all_namespaces = true;
                            changed = true;
                        }
                        for name in self.namespaces.clone() {
                            let on = !self.all_namespaces && self.namespace == name;
                            if ui.selectable_label(on, &name).clicked() {
                                self.all_namespaces = false;
                                self.namespace = name;
                                changed = true;
                            }
                        }
                    });
            }
            egui::ComboBox::from_id_salt("k8s_kind")
                .selected_text(KINDS[self.kind].0)
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (i, (label, _, _)) in KINDS.iter().enumerate() {
                        if ui.selectable_label(self.kind == i, *label).clicked() && self.kind != i {
                            self.kind = i;
                            self.selected = None;
                            changed = true;
                        }
                    }
                });
            ui::search_field(ui, &mut self.filter, "Filtrer", 180.0);
            if ui::icon_button(ui, "refresh", "Actualiser").clicked() {
                self.load_namespaces();
                changed = true;
            }
            if self.list_job.is_some() {
                ui.spinner();
            }
            if changed {
                self.table = Table::default();
                self.detail = None;
                self.editor = None;
                self.reload();
            }
        });
    }

    fn actions_ui(&mut self, ui: &mut egui::Ui) {
        let Some((ns, name)) = self.selected.clone() else {
            return;
        };
        let resource = KINDS[self.kind].1;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).strong());
            if ui::button(ui, "Describe").clicked() {
                self.describe(&ns, &name);
            }
            if ui::button(ui, "YAML").clicked() {
                self.yaml(&ns, &name);
            }
            if self.can_edit() && ui::button(ui, "Éditer").clicked() {
                self.open_editor(&ns, &name);
            }
            if resource == "pods" && ui::button(ui, "Logs").clicked() {
                self.logs(&ns, &name);
            }
            if self.can_restart() && ui::button(ui, "Redémarrer").clicked() {
                self.ask_restart();
            }
            if ui::button(ui, "Supprimer").clicked() {
                self.confirm = Some(Confirm {
                    title: "Supprimer".into(),
                    text: format!("Supprimer {resource}/{name} du cluster ?"),
                    label: format!("Suppression de {name}"),
                    args: strings(&["delete", resource, &name]),
                });
            }
        });
    }

    fn table_ui(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui.ctx());
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(t.deleted));
            return;
        }
        if self.table.headers.is_empty() {
            let text = if self.list_job.is_some() || !self.meta_loaded {
                "Chargement…"
            } else {
                "Aucune ressource"
            };
            ui.label(RichText::new(text).color(t.text_secondary));
            return;
        }
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<Vec<String>> = self
            .table
            .rows
            .iter()
            .filter(|r| filter.is_empty() || r.iter().any(|c| c.to_lowercase().contains(&filter)))
            .cloned()
            .collect();
        if let Some((header, asc)) = &self.sort
            && let Some(col) = self.table.headers.iter().position(|h| h == header)
        {
            rows.sort_by(|a, b| {
                let order = compare_cells(header, &a[col], &b[col]);
                if *asc { order } else { order.reverse() }
            });
        }
        let sort = self.sort.clone();
        let mut sort_clicked = None;
        let name_col = usize::from(self.namespaced() && self.all_namespaces);
        let text_color = ui.visuals().text_color();
        let mut clicked = None;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("k8s_table")
                .striped(true)
                .num_columns(self.table.headers.len())
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    for header in &self.table.headers {
                        let arrow = match &sort {
                            Some((h, asc)) if h == header => Some(if *asc { "chevronUp" } else { "chevronDown" }),
                            _ => None,
                        };
                        let response = ui
                            .horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let label = ui.add(
                                    egui::Label::new(
                                        RichText::new(header)
                                            .font(ui::semibold(12.0))
                                            .color(t.text_secondary),
                                    )
                                    .sense(egui::Sense::click()),
                                );
                                if let Some(name) = arrow {
                                    ui.add(ui::icon(ui, name));
                                }
                                label
                            })
                            .inner
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            sort_clicked = Some(header.clone());
                        }
                    }
                    ui.end_row();
                    for row in &rows {
                        let identity = self.identity(row);
                        for (c, cell) in row.iter().enumerate() {
                            if c == name_col {
                                let on = identity.is_some() && identity == self.selected;
                                if ui.selectable_label(on, cell).clicked() {
                                    clicked = identity.clone();
                                }
                            } else {
                                ui.label(RichText::new(cell).color(cell_color(&t, cell, text_color)));
                            }
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(identity) = clicked {
            self.selected = Some(identity);
        }
        if let Some(header) = sort_clicked {
            // First click ascending, then descending, then back to the order of kubectl.
            self.sort = match self.sort.take() {
                Some((h, true)) if h == header => Some((h, false)),
                Some((h, false)) if h == header => None,
                _ => Some((header, true)),
            };
        }
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui.ctx());
        let Some(detail) = &mut self.detail else {
            return;
        };
        ui.horizontal(|ui| {
            ui::section(ui, &detail.title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button(ui, "close", "Fermer").clicked() {
                    detail.title.clear();
                }
                if ui::icon_button(ui, "copy", "Copier").clicked() {
                    ui.ctx().copy_text(detail.text.clone());
                }
            });
        });
        let color = if detail.error { t.deleted } else { ui.visuals().text_color() };
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut detail.text.as_str())
                    .font(egui::TextStyle::Monospace)
                    .text_color(color)
                    .desired_width(f32::INFINITY)
                    .frame(false),
            )
            .on_hover_cursor(egui::CursorIcon::Text);
        });
        if detail.title.is_empty() {
            self.detail = None;
            self.detail_job = None;
        }
    }

    fn editor_ui(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui.ctx());
        let saving = self.saving;
        let Some(editor) = &mut self.editor else {
            return;
        };
        let (mut save, mut close) = (false, false);
        ui.horizontal(|ui| {
            ui::section(ui, &format!("Éditer {}/{}", editor.resource, editor.name));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button(ui, "close", "Fermer sans enregistrer").clicked() {
                    close = true;
                }
                if ui.add_enabled_ui(!saving, |ui| ui::primary_button(ui, "Enregistrer")).inner.clicked() {
                    save = true;
                }
            });
        });
        if editor.resource == "secrets" {
            ui.label(RichText::new("Les valeurs sont affichées décodées.").color(t.text_secondary));
        }
        if !editor.binary.is_empty() {
            ui.label(
                RichText::new(format!("Valeurs binaires conservées : {}", editor.binary.join(", ")))
                    .color(t.text_secondary),
            );
        }
        let mut remove = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, (key, value)) in editor.entries.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(key.as_str()).font(ui::semibold(12.5)));
                    if ui::icon_button(ui, "delete", "Supprimer la clé").clicked() {
                        remove = Some(i);
                    }
                });
                let rows = value.lines().count().clamp(2, 14);
                ui.add(
                    egui::TextEdit::multiline(value)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY)
                        .code_editor(),
                )
                .on_hover_cursor(egui::CursorIcon::Text);
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut editor.new_key)
                        .hint_text("Nouvelle clé")
                        .desired_width(200.0),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui::button(ui, "Ajouter").clicked() || enter {
                    editor.add_key();
                }
            });
            if let Some(error) = &editor.error {
                ui.label(RichText::new(error).color(t.deleted));
            }
        });
        if let Some(i) = remove {
            editor.entries.remove(i);
        }
        if close {
            self.editor = None;
        } else if save {
            self.save_editor();
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm else {
            return;
        };
        let mut result = None;
        egui::Window::new(&confirm.title)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui::dialog_title(ui, &confirm.title);
                ui.label(&confirm.text);
                let (ok, cancel) = ui::dialog_buttons(ui, &confirm.title);
                if ok {
                    result = Some(true);
                } else if cancel {
                    result = Some(false);
                }
            });
        if let Some(ok) = result
            && let Some(confirm) = self.confirm.take()
            && ok
        {
            let ns = self.selected.as_ref().map(|s| s.0.clone()).unwrap_or_default();
            self.run_action(confirm.label, confirm.args, &ns);
        }
    }
}

/// Color of a status cell: green when healthy, red when failing.
fn cell_color(theme: &ui::Theme, cell: &str, default: Color32) -> Color32 {
    match cell {
        "Running" | "Active" | "Ready" | "Complete" | "Completed" | "Bound" => theme.added,
        "Pending" | "ContainerCreating" | "Terminating" | "NotReady" => theme.modified,
        c if c.contains("Error")
            || c.contains("BackOff")
            || c.contains("Failed")
            || c.contains("Evicted")
            || c == "OOMKilled" =>
        {
            theme.deleted
        }
        _ => default,
    }
}

impl BuiltinPlugin for K8sPlugin {
    fn id(&self) -> &'static str {
        "k8s"
    }
    fn name(&self) -> &'static str {
        "Kubernetes"
    }
    fn description(&self) -> &'static str {
        "Ressources d'un cluster Kubernetes : pods, deployments, services… describe, YAML, logs, redémarrage et suppression, avec kubectl."
    }
    fn icon(&self) -> &'static str {
        "k8s/icon"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("k8s")
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        self.poll(ctx, host);
    }

    fn shortcuts(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::Num8)) {
            host.show_tool_window(&TOOL_WINDOWS[0]);
        }
    }

    fn tool_window(&mut self, id: &str, ui: &mut egui::Ui, _host: &mut dyn Host) {
        if id != TOOL_WINDOWS[0].id {
            return;
        }
        self.drawn_at = Some(Instant::now());
        // The panel is as tall as its content: fill it, or it shrinks (and cannot be resized)
        // while the list is loading, in error or empty.
        ui.set_min_height(ui.available_height());
        self.toolbar_ui(ui);
        self.actions_ui(ui);
        ui.separator();
        if self.detail.is_some() || self.editor.is_some() {
            egui::SidePanel::right("k8s_detail")
                .resizable(true)
                .default_width(ui.available_width() * 0.5)
                .show_inside(ui, |ui| {
                    if self.editor.is_some() {
                        self.editor_ui(ui);
                    } else {
                        self.detail_ui(ui);
                    }
                });
        }
        self.table_ui(ui);
        self.confirm_window(ui.ctx());
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: "k8s.open",
                label: "Kubernetes : ressources  Alt+8".into(),
            },
            Command {
                id: "k8s.restart",
                label: "Kubernetes : rollout restart de la ressource sélectionnée".into(),
            },
            Command {
                id: "k8s.refresh",
                label: "Kubernetes : actualiser".into(),
            },
        ]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "k8s.open" => host.show_tool_window(&TOOL_WINDOWS[0]),
            "k8s.restart" => {
                host.show_tool_window(&TOOL_WINDOWS[0]);
                if self.selected.is_some() && self.can_restart() {
                    self.ask_restart();
                } else {
                    host.notify(
                        "Sélectionnez un pod, deployment, statefulset ou daemonset dans la fenêtre Kubernetes"
                            .into(),
                    );
                }
            }
            "k8s.refresh" => {
                self.load_namespaces();
                self.reload();
                host.show_tool_window(&TOOL_WINDOWS[0]);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct NoHost(PathBuf);
    impl Host for NoHost {
        fn root(&self) -> &Path {
            &self.0
        }
        fn active_file(&self) -> Option<PathBuf> {
            None
        }
        fn open_file(&mut self, _: PathBuf) {}
        fn open_project(&mut self, _: PathBuf) {}
        fn save_files(&mut self) -> bool {
            true
        }
        fn reload_files(&mut self) {}
        fn show_tool_window(&mut self, _: &'static ToolWindow) {}
        fn notify(&mut self, _: String) {}
    }

    #[test]
    fn parses_kubectl_columns() {
        let text = "NAME         READY   STATUS             RESTARTS      AGE   NOMINATED NODE\n\
                    web-1        1/1     Running            0             3d    <none>\n\
                    job-x        0/1     CrashLoopBackOff   5 (2m ago)    1h    <none>\n";
        let table = parse_table(text);
        assert_eq!(
            table.headers,
            ["NAME", "READY", "STATUS", "RESTARTS", "AGE", "NOMINATED NODE"]
        );
        assert_eq!(table.rows[1][2], "CrashLoopBackOff");
        assert_eq!(table.rows[1][3], "5 (2m ago)");
        assert_eq!(table.rows[0][5], "<none>");
        assert_eq!(parse_table(""), Table::default());
    }

    #[test]
    fn sorts_cells_by_column_type() {
        use std::cmp::Ordering::{Greater, Less};
        assert_eq!(compare_cells("AGE", "5m", "1h"), Less);
        assert_eq!(compare_cells("AGE", "2d3h", "2d"), Greater);
        assert_eq!(compare_cells("RESTARTS", "10 (2m ago)", "9"), Greater);
        assert_eq!(compare_cells("READY", "1/3", "2/2"), Less);
        assert_eq!(compare_cells("NAME", "api", "Web"), Less);
    }

    #[test]
    fn secrets_are_edited_decoded_and_sent_as_string_data() {
        let doc = serde_json::json!({
            "kind": "Secret",
            "metadata": {"name": "db", "namespace": "dev", "managedFields": []},
            "data": {"user": "YWRtaW4=", "blob": "//79"}
        });
        let mut editor = Editor::new("secrets", doc).unwrap();
        assert_eq!(editor.entries, [("user".to_string(), "admin".to_string())]);
        assert_eq!(editor.binary, ["blob"]);
        editor.entries[0].1 = "root".into();
        editor.new_key = "port".into();
        editor.add_key();
        let out = editor.document();
        assert_eq!(out["stringData"]["user"], "root");
        assert_eq!(out["stringData"]["port"], "");
        assert_eq!(out["data"]["blob"], "//79");
        assert!(out["data"].get("user").is_none());
        assert!(out["metadata"].get("managedFields").is_none());
    }

    #[test]
    fn config_maps_keep_binary_data_and_validate_keys() {
        let doc = serde_json::json!({
            "metadata": {"name": "cfg"}, "data": {"a": "1"}, "binaryData": {"b": "AAEC"}
        });
        let mut editor = Editor::new("configmaps", doc).unwrap();
        editor.new_key = "bad key".into();
        editor.add_key();
        assert!(editor.error.is_some());
        editor.new_key = "b".into();
        editor.add_key();
        assert!(editor.error.is_some(), "binary keys are taken");
        editor.entries.clear();
        assert_eq!(editor.document()["data"], serde_json::json!({}));
        assert_eq!(editor.document()["binaryData"]["b"], "AAEC");
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(b64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(b64_decode("aGk").unwrap(), b"hi");
        assert!(b64_decode("a*b").is_none());
    }

    #[test]
    fn row_identity_follows_the_namespace_column() {
        let mut plugin = K8sPlugin::default();
        let row = vec!["kube-system".to_string(), "dns".to_string(), "1/1".to_string()];
        plugin.all_namespaces = true;
        assert_eq!(plugin.identity(&row), Some(("kube-system".into(), "dns".into())));
        plugin.all_namespaces = false;
        plugin.namespace = "dev".into();
        let row = vec!["dns".to_string()];
        assert_eq!(plugin.identity(&row), Some(("dev".into(), "dns".into())));
    }

    #[test]
    fn window_renders() {
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut host = NoHost(PathBuf::new());
        let mut plugin = K8sPlugin::default();
        plugin.table = parse_table("NAME   READY\nweb   1/1\n");
        plugin.meta_loaded = true;
        plugin.selected = Some((String::new(), "web".into()));
        plugin.detail = Some(Detail {
            title: "web.yaml".into(),
            text: "kind: Pod".into(),
            error: false,
        });
        plugin.detail = None;
        plugin.editor = Editor::new(
            "configmaps",
            serde_json::json!({"metadata": {"name": "cfg"}, "data": {"a": "1\n2"}}),
        )
        .ok();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plugin.tool_window("k8s.resources", ui, &mut host)
                });
            });
        }
        assert!(plugin.editor.is_some());
        plugin.confirm = Some(Confirm {
            title: "Supprimer".into(),
            text: "?".into(),
            label: "x".into(),
            args: Vec::new(),
        });
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plugin.tool_window("k8s.resources", ui, &mut host)
                });
            });
        }
        assert!(plugin.confirm.is_some());
    }
}
