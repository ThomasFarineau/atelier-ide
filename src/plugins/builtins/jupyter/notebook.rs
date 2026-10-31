//! Jupyter notebooks (`.ipynb`): the cells drawn one under the other, code cells edited and run
//! in a kernel of the Python of the project, their outputs (text, errors, images, Markdown)
//! written back into the notebook as Jupyter does.

use super::kernel::{Event, Kernel};
use crate::ui;
use eframe::egui::{self, FontId, Margin, RichText, Stroke};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

pub fn is_notebook(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ipynb"))
}

// ---- The notebook as JSON ------------------------------------------------------------------

/// A notebook: an object with a list of cells.
pub fn parse(text: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(text).ok()?;
    value["cells"].is_array().then_some(value)
}

/// The text of a notebook as Jupyter writes it: one space of indentation, a last newline.
pub fn to_text(notebook: &Value) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    if serde::Serialize::serialize(notebook, &mut serializer).is_err() {
        return String::new();
    }
    let mut text = String::from_utf8(out).unwrap_or_default();
    text.push('\n');
    text
}

/// A text of a notebook: a string, or a list of lines to join.
pub fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(lines) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// A text as Jupyter stores it: its lines, each with its newline.
pub fn lines(text: &str) -> Value {
    Value::Array(
        text.split_inclusive('\n')
            .map(|l| Value::String(l.to_string()))
            .collect(),
    )
}

pub fn source(cell: &Value) -> String {
    text_of(&cell["source"])
}

/// An empty cell of `kind` (`code` or `markdown`); notebooks of format 4.5 and later give
/// each cell an id.
pub fn new_cell(kind: &str, with_id: bool) -> Value {
    let mut cell = json!({"cell_type": kind, "metadata": {}, "source": []});
    if kind == "code" {
        cell["execution_count"] = Value::Null;
        cell["outputs"] = json!([]);
    }
    if with_id {
        let random = std::hash::BuildHasher::hash_one(&std::hash::RandomState::new(), 0u8);
        cell["id"] = json!(format!("{:08x}", random as u32));
    }
    cell
}

fn has_ids(notebook: &Value) -> bool {
    notebook["nbformat"].as_u64().unwrap_or(4) > 4
        || notebook["nbformat_minor"].as_u64().unwrap_or(0) >= 5
}

/// The language of the code cells, for highlighting.
pub fn language(notebook: &Value) -> String {
    let metadata = &notebook["metadata"];
    metadata["language_info"]["name"]
        .as_str()
        .or_else(|| metadata["kernelspec"]["language"].as_str())
        .unwrap_or("python")
        .to_string()
}

/// A new, empty notebook of Python.
pub fn empty() -> Value {
    json!({
        "cells": [new_cell("code", true)],
        "metadata": {
            "kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"},
            "language_info": {"name": "python"}
        },
        "nbformat": 4,
        "nbformat_minor": 5
    })
}

/// Adds what the kernel sent for a cell to its outputs, as Jupyter does: the text of a stream
/// goes on with the previous one of the same stream.
pub fn apply_output(cell: &mut Value, kind: &str, content: &Value) {
    if !cell["outputs"].is_array() {
        cell["outputs"] = json!([]);
    }
    let outputs = cell["outputs"].as_array_mut().expect("array");
    match kind {
        "clear_output" => outputs.clear(),
        "stream" => {
            let name = content["name"].as_str().unwrap_or("stdout");
            let text = content["text"].as_str().unwrap_or_default();
            if let Some(last) = outputs.last_mut()
                && last["output_type"] == "stream"
                && last["name"] == name
            {
                let joined = text_of(&last["text"]) + text;
                last["text"] = lines(&joined);
            } else {
                outputs.push(json!({"output_type": "stream", "name": name, "text": lines(text)}));
            }
        }
        "display_data" => outputs.push(json!({
            "output_type": "display_data",
            "data": content["data"],
            "metadata": content.get("metadata").cloned().unwrap_or_else(|| json!({})),
        })),
        "execute_result" => outputs.push(json!({
            "output_type": "execute_result",
            "execution_count": content["execution_count"],
            "data": content["data"],
            "metadata": content.get("metadata").cloned().unwrap_or_else(|| json!({})),
        })),
        "error" => outputs.push(json!({
            "output_type": "error",
            "ename": content["ename"],
            "evalue": content["evalue"],
            "traceback": content["traceback"],
        })),
        _ => {}
    }
}

/// A text without its ANSI colors (the tracebacks of IPython have some).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Base64, as images are stored in a notebook (line breaks allowed).
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

// ---- The view -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Off,
    Starting,
    Ready(String),
    /// Jupyter is not installed in the Python of the project.
    Missing,
    Failed(String),
}

/// What the view asks of the IDE, done by the plugin with the host.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    InstallJupyter,
    OpenJupyterLab(PathBuf),
    ShowRuntimes,
}

/// A notebook open in the editor: its cells, its kernel.
#[derive(Default)]
pub struct View {
    hash: u64,
    notebook: Option<Value>,
    /// A key per cell that stays the same when cells move, for the outputs of a running cell.
    keys: Vec<u64>,
    next_key: u64,
    /// Markdown cells being edited.
    editing: HashSet<u64>,
    /// The cell whose editor has the keyboard.
    focused: Option<u64>,
    /// Cells sent to the kernel and not done yet, the running one first.
    queued: Vec<u64>,
    kernel: Option<Kernel>,
    pub status: Status,
    cache: CommonMarkCache,
}

fn hash(text: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// What the user did in a frame, applied after drawing.
enum Action {
    Run(u64),
    RunAll,
    Interrupt,
    Restart,
    ClearOutputs,
    Add(usize, &'static str),
    Delete(u64),
    Move(u64, isize),
    ChangeType(u64, &'static str),
}

impl View {
    fn reload(&mut self, text: &str) {
        self.hash = hash(text);
        self.notebook = parse(text);
        let count = self
            .notebook
            .as_ref()
            .map_or(0, |n| n["cells"].as_array().map_or(0, Vec::len));
        if self.keys.len() != count {
            self.keys = (0..count as u64).map(|i| self.next_key + i).collect();
            self.next_key += count as u64;
            self.queued.clear();
        }
    }

    fn index(&self, key: u64) -> Option<usize> {
        self.keys.iter().position(|k| *k == key)
    }

    fn cells_mut(&mut self) -> Option<&mut Vec<Value>> {
        self.notebook.as_mut()?["cells"].as_array_mut()
    }

    fn write(&mut self, text: &mut String) {
        if let Some(notebook) = &self.notebook {
            *text = to_text(notebook);
            self.hash = hash(text);
        }
    }

    fn ensure_kernel(&mut self, ctx: &egui::Context, root: &Path, path: &Path) {
        if self.kernel.is_some() {
            return;
        }
        let python = crate::plugins::builtins::runtimes::install::python(root)
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
        let cwd = path.parent().unwrap_or(root);
        let ctx = ctx.clone();
        match Kernel::start(&python, cwd, move || ctx.request_repaint()) {
            Ok(kernel) => {
                self.kernel = Some(kernel);
                self.status = Status::Starting;
            }
            Err(e) => {
                self.status = Status::Failed(format!("{}: {e}", python.display()));
            }
        }
    }

    fn run(&mut self, key: u64, ctx: &egui::Context, root: &Path, path: &Path) {
        let Some(index) = self.index(key) else {
            return;
        };
        let Some(cell) = self.cells_mut().and_then(|c| c.get_mut(index)) else {
            return;
        };
        if cell["cell_type"] != "code" {
            self.editing.remove(&key);
            return;
        }
        let code = source(cell);
        cell["outputs"] = json!([]);
        cell["execution_count"] = Value::Null;
        self.ensure_kernel(ctx, root, path);
        if let Some(kernel) = &mut self.kernel {
            kernel.execute(key, &code);
            self.queued.push(key);
        }
    }

    /// The events of the kernel; true when the notebook changed.
    fn poll(&mut self) -> bool {
        let Some(kernel) = &self.kernel else {
            return false;
        };
        let mut changed = false;
        for event in kernel.events() {
            match event {
                Event::Ready(version) => self.status = Status::Ready(version),
                Event::Missing => {
                    self.status = Status::Missing;
                    self.queued.clear();
                    self.kernel = None;
                    return changed;
                }
                Event::Failed(why) => {
                    let why = match why.trim() {
                        "" => tr!("jupyter.kernel_stopped").to_string(),
                        why => why.lines().last().unwrap_or(why).to_string(),
                    };
                    self.status = Status::Failed(why);
                    self.queued.clear();
                    self.kernel = None;
                    return changed;
                }
                Event::Output {
                    cell,
                    kind,
                    content,
                } => {
                    if let Some(index) = self.index(cell)
                        && let Some(cell) = self.cells_mut().and_then(|c| c.get_mut(index))
                    {
                        apply_output(cell, &kind, &content);
                        changed = true;
                    }
                }
                Event::Done { cell, count } => {
                    self.queued.retain(|k| *k != cell);
                    if let Some(index) = self.index(cell)
                        && let Some(cell) = self.cells_mut().and_then(|c| c.get_mut(index))
                    {
                        cell["execution_count"] = count.map_or(Value::Null, Value::from);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        root: &Path,
        path: &Path,
        text: &mut String,
        requests: &mut Vec<Request>,
    ) {
        if hash(text) != self.hash {
            self.reload(text);
        }
        if self.poll() {
            self.write(text);
        }
        let t = ui::theme(ui.ctx());
        let Some(notebook) = self.notebook.clone() else {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("jupyter.notebook_invalid")).color(t.error));
            });
            return;
        };
        let mut actions = Vec::new();
        self.toolbar(ui, path, &mut actions, requests);
        ui.separator();
        let language = language(&notebook);
        let cells = notebook["cells"].as_array().cloned().unwrap_or_default();
        let mut edited: Vec<(usize, String)> = Vec::new();
        let width = ui.available_width();
        egui::ScrollArea::vertical()
            .id_salt(("notebook", path))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(20, 12))
                    .show(ui, |ui| {
                        ui.set_max_width((width - 40.0).max(200.0));
                        for (index, cell) in cells.iter().enumerate() {
                            let key = self.keys.get(index).copied().unwrap_or_default();
                            ui.push_id(("cell", key), |ui| {
                                if let Some(new) = self.cell(ui, key, cell, &language, &mut actions)
                                {
                                    edited.push((index, new));
                                }
                            });
                            ui.add_space(10.0);
                        }
                        ui.horizontal(|ui| {
                            if ui::icon_text_button(ui, Some("add"), tr!("jupyter.add_code"), false)
                                .clicked()
                            {
                                actions.push(Action::Add(cells.len(), "code"));
                            }
                            if ui::icon_text_button(
                                ui,
                                Some("add"),
                                tr!("jupyter.add_markdown"),
                                false,
                            )
                            .clicked()
                            {
                                actions.push(Action::Add(cells.len(), "markdown"));
                            }
                        });
                    });
            });
        let mut changed = false;
        if let Some(cells) = self.cells_mut() {
            for (index, new) in edited {
                if let Some(cell) = cells.get_mut(index) {
                    cell["source"] = lines(&new);
                    changed = true;
                }
            }
        }
        let ctx = ui.ctx().clone();
        for action in actions {
            changed |= self.apply(action, &ctx, root, path);
        }
        if changed {
            self.write(text);
        }
        if !self.queued.is_empty() || self.status == Status::Starting {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        actions: &mut Vec<Action>,
        requests: &mut Vec<Request>,
    ) {
        let t = ui::theme(ui.ctx());
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if ui::icon_button(ui, "run", tr!("jupyter.run_all")).clicked() {
                actions.push(Action::RunAll);
            }
            let running = self.kernel.is_some();
            if ui::icon_button_enabled(ui, "stop", running, tr!("jupyter.interrupt")).clicked() {
                actions.push(Action::Interrupt);
            }
            if ui::icon_button_enabled(ui, "rerun", running, tr!("jupyter.restart")).clicked() {
                actions.push(Action::Restart);
            }
            if ui::icon_button(ui, "delete", tr!("jupyter.clear_outputs")).clicked() {
                actions.push(Action::ClearOutputs);
            }
            if ui::icon_button(ui, "openNewTab", tr!("jupyter.open_jupyterlab")).clicked() {
                requests.push(Request::OpenJupyterLab(path.to_path_buf()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                match &self.status {
                    Status::Missing => {
                        if ui::primary_button(ui, tr!("jupyter.install_jupyter")).clicked() {
                            requests.push(Request::InstallJupyter);
                            self.status = Status::Off;
                        }
                        ui.label(
                            RichText::new(tr!("jupyter.jupyter_missing"))
                                .small()
                                .color(t.warning),
                        );
                    }
                    Status::Failed(why) => {
                        if ui::button(ui, tr!("jupyter.show_runtimes")).clicked() {
                            requests.push(Request::ShowRuntimes);
                        }
                        ui.label(RichText::new(why).small().color(t.error))
                            .on_hover_text(why);
                    }
                    status => {
                        let label = match status {
                            Status::Starting => tr!("jupyter.kernel_starting").to_string(),
                            Status::Ready(version) if !self.queued.is_empty() => {
                                trf!("jupyter.kernel_busy", version = version)
                            }
                            Status::Ready(version) => {
                                trf!("jupyter.kernel_idle", version = version)
                            }
                            _ => tr!("jupyter.kernel_off").to_string(),
                        };
                        ui.label(RichText::new(label).small().color(t.text_secondary));
                    }
                }
            });
        });
    }

    /// Draws a cell; Some with its new source when it was edited.
    fn cell(
        &mut self,
        ui: &mut egui::Ui,
        key: u64,
        cell: &Value,
        language: &str,
        actions: &mut Vec<Action>,
    ) -> Option<String> {
        let t = ui::theme(ui.ctx());
        let kind = cell["cell_type"].as_str().unwrap_or("raw");
        let mut source = source(cell);
        let mut edited = None;
        // The header: run, execution count, and the actions of the cell.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if kind == "code" {
                if ui::icon_button(ui, "run", tr!("jupyter.run_cell")).clicked() {
                    actions.push(Action::Run(key));
                }
                let count = if self.queued.first() == Some(&key) {
                    "[*]".to_string()
                } else if self.queued.contains(&key) {
                    "[…]".to_string()
                } else {
                    match cell["execution_count"].as_u64() {
                        Some(n) => format!("[{n}]"),
                        None => "[ ]".to_string(),
                    }
                };
                ui.label(
                    RichText::new(count)
                        .font(FontId::monospace(12.0))
                        .color(t.text_secondary),
                );
            } else {
                ui.label(RichText::new(kind).small().color(t.text_secondary));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if ui::icon_button(ui, "delete", tr!("jupyter.delete_cell")).clicked() {
                    actions.push(Action::Delete(key));
                }
                if ui::icon_button(ui, "down", tr!("jupyter.move_down")).clicked() {
                    actions.push(Action::Move(key, 1));
                }
                if ui::icon_button(ui, "up", tr!("jupyter.move_up")).clicked() {
                    actions.push(Action::Move(key, -1));
                }
                let other = if kind == "code" { "markdown" } else { "code" };
                let label = if other == "code" {
                    tr!("jupyter.to_code")
                } else {
                    tr!("jupyter.to_markdown")
                };
                if ui::icon_button(ui, "changelist", label).clicked() {
                    actions.push(Action::ChangeType(key, other));
                }
            });
        });
        let editing = kind != "markdown" || self.editing.contains(&key);
        if editing {
            // Shift+Enter runs the cell, as in Jupyter.
            if self.focused == Some(key)
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter))
            {
                actions.push(Action::Run(key));
            }
            let syntax = if kind == "code" { language } else { "md" };
            let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
            let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                let mut job = egui_extras::syntax_highlighting::highlight(
                    ui.ctx(),
                    ui.style(),
                    &theme,
                    buffer.as_str(),
                    syntax,
                );
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let response = egui::Frame::new()
                .fill(t.field)
                .stroke(Stroke::new(
                    1.0_f32,
                    if self.focused == Some(key) {
                        t.accent
                    } else {
                        t.border
                    },
                ))
                .corner_radius(4)
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut source)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .frame(false)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layouter),
                    )
                })
                .inner;
            if response.has_focus() {
                self.focused = Some(key);
            } else if self.focused == Some(key) && !response.has_focus() {
                self.focused = None;
            }
            if response.changed() {
                edited = Some(source.clone());
            }
            if kind == "markdown" && ui::button(ui, tr!("jupyter.done_editing")).clicked() {
                self.editing.remove(&key);
            }
        } else {
            let response = egui::Frame::new()
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if source.trim().is_empty() {
                        ui.label(
                            RichText::new(tr!("jupyter.empty_markdown"))
                                .italics()
                                .color(t.text_secondary),
                        );
                    } else {
                        CommonMarkViewer::new().show(ui, &mut self.cache, &source);
                    }
                })
                .response
                .interact(egui::Sense::click());
            if response.double_clicked() {
                self.editing.insert(key);
            }
            response.on_hover_text(tr!("jupyter.double_click_edit"));
        }
        if kind == "code"
            && let Some(outputs) = cell["outputs"].as_array()
        {
            for (index, output) in outputs.iter().enumerate() {
                ui.push_id(("output", index), |ui| self.output(ui, output));
            }
        }
        edited
    }

    fn output(&mut self, ui: &mut egui::Ui, output: &Value) {
        let t = ui::theme(ui.ctx());
        let mono = |ui: &mut egui::Ui, text: &str, color| {
            ui.add(
                egui::Label::new(
                    RichText::new(text.trim_end_matches('\n'))
                        .font(FontId::monospace(12.5))
                        .color(color),
                )
                .selectable(true)
                .wrap(),
            );
        };
        egui::Frame::new()
            .inner_margin(Margin {
                left: 10,
                right: 4,
                top: 4,
                bottom: 2,
            })
            .show(ui, |ui| match output["output_type"].as_str() {
                Some("stream") => {
                    let color = if output["name"] == "stderr" {
                        t.warning
                    } else {
                        t.text
                    };
                    mono(ui, &strip_ansi(&text_of(&output["text"])), color);
                }
                Some("error") => {
                    let traceback: Vec<String> = output["traceback"]
                        .as_array()
                        .map(|l| l.iter().filter_map(Value::as_str).map(strip_ansi).collect())
                        .unwrap_or_default();
                    let text = if traceback.is_empty() {
                        format!(
                            "{}: {}",
                            output["ename"].as_str().unwrap_or_default(),
                            output["evalue"].as_str().unwrap_or_default()
                        )
                    } else {
                        traceback.join("\n")
                    };
                    mono(ui, &text, t.error);
                }
                Some("execute_result" | "display_data") => {
                    let data = &output["data"];
                    for (mime, extension) in [("image/png", "png"), ("image/jpeg", "jpg")] {
                        if let Some(bytes) = data.get(mime).and_then(|v| base64_decode(&text_of(v)))
                        {
                            let uri = format!("bytes://ipynb/{:016x}.{extension}", {
                                let mut h = std::hash::DefaultHasher::new();
                                bytes.hash(&mut h);
                                h.finish()
                            });
                            let max = ui.available_width().min(900.0);
                            ui.add(egui::Image::from_bytes(uri, bytes).max_width(max));
                            return;
                        }
                    }
                    if let Some(svg) = data.get("image/svg+xml") {
                        let svg = text_of(svg);
                        let uri = format!("bytes://ipynb/{:016x}.svg", hash(&svg));
                        ui.add(egui::Image::from_bytes(uri, svg.into_bytes()).max_width(900.0));
                    } else if let Some(markdown) = data.get("text/markdown") {
                        CommonMarkViewer::new().show(ui, &mut self.cache, &text_of(markdown));
                    } else if let Some(plain) = data.get("text/plain") {
                        mono(ui, &strip_ansi(&text_of(plain)), t.text);
                    } else if let Some(mime) = data.as_object().and_then(|m| m.keys().next()) {
                        ui.label(
                            RichText::new(trf!("jupyter.output_unsupported", mime = mime))
                                .italics()
                                .color(t.text_secondary),
                        );
                    }
                }
                _ => {}
            });
        ui.add_space(2.0);
    }

    /// Applies an action; true when the notebook changed.
    fn apply(&mut self, action: Action, ctx: &egui::Context, root: &Path, path: &Path) -> bool {
        match action {
            Action::Run(key) => {
                self.run(key, ctx, root, path);
                true
            }
            Action::RunAll => {
                let code: Vec<u64> = self
                    .notebook
                    .as_ref()
                    .and_then(|n| n["cells"].as_array())
                    .map(|cells| {
                        cells
                            .iter()
                            .zip(&self.keys)
                            .filter(|(c, _)| c["cell_type"] == "code")
                            .map(|(_, k)| *k)
                            .collect()
                    })
                    .unwrap_or_default();
                for key in code {
                    self.run(key, ctx, root, path);
                }
                true
            }
            Action::Interrupt => {
                if let Some(kernel) = &mut self.kernel {
                    kernel.interrupt();
                }
                false
            }
            Action::Restart => {
                self.kernel = None;
                self.queued.clear();
                self.status = Status::Off;
                self.ensure_kernel(ctx, root, path);
                false
            }
            Action::ClearOutputs => {
                for cell in self.cells_mut().into_iter().flatten() {
                    if cell["cell_type"] == "code" {
                        cell["outputs"] = json!([]);
                        cell["execution_count"] = Value::Null;
                    }
                }
                true
            }
            Action::Add(index, kind) => {
                let with_id = self.notebook.as_ref().is_some_and(has_ids);
                let key = self.next_key;
                self.next_key += 1;
                let Some(cells) = self.cells_mut() else {
                    return false;
                };
                let index = index.min(cells.len());
                cells.insert(index, new_cell(kind, with_id));
                self.keys.insert(index, key);
                if kind == "markdown" {
                    self.editing.insert(key);
                }
                true
            }
            Action::Delete(key) => {
                let Some(index) = self.index(key) else {
                    return false;
                };
                if let Some(cells) = self.cells_mut() {
                    cells.remove(index);
                }
                self.keys.remove(index);
                self.queued.retain(|k| *k != key);
                true
            }
            Action::Move(key, by) => {
                let Some(index) = self.index(key) else {
                    return false;
                };
                let target = index as isize + by;
                if target < 0 || target as usize >= self.keys.len() {
                    return false;
                }
                let target = target as usize;
                if let Some(cells) = self.cells_mut() {
                    cells.swap(index, target);
                }
                self.keys.swap(index, target);
                true
            }
            Action::ChangeType(key, kind) => {
                let with_id = self.notebook.as_ref().is_some_and(has_ids);
                let Some(index) = self.index(key) else {
                    return false;
                };
                let Some(cell) = self.cells_mut().and_then(|c| c.get_mut(index)) else {
                    return false;
                };
                let mut new = new_cell(kind, false);
                new["source"] = cell["source"].clone();
                if with_id && let Some(id) = cell.get("id") {
                    new["id"] = id.clone();
                }
                *cell = new;
                if kind == "markdown" {
                    self.editing.insert(key);
                }
                true
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/jupyter/notebook/tests.rs"]
mod tests;
