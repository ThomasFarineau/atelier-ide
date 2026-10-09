//! The Problems tool window: the errors and warnings of the open documents, grouped by file,
//! as IntelliJ lists them (Alt+6).

use crate::{
    plugins::builtin::{BuiltinPlugin, Dock, Host, Problem, ToolWindow},
    ui,
};
use eframe::egui::{self, Color32, FontId, RichText};
use std::{collections::HashSet, path::PathBuf};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "problems.list",
    title: "Problèmes",
    icon: "problems",
    dock: Dock::Bottom,
    shortcut: "Alt+6",
}];

const ROW_HEIGHT: f32 = 22.0;

pub struct ProblemsPlugin {
    errors: bool,
    warnings: bool,
    collapsed: HashSet<PathBuf>,
    selected: Option<(PathBuf, usize, usize)>,
}

impl Default for ProblemsPlugin {
    fn default() -> Self {
        Self {
            errors: true,
            warnings: true,
            collapsed: HashSet::new(),
            selected: None,
        }
    }
}

enum Row<'a> {
    File(&'a PathBuf, usize),
    Item(&'a Problem),
}

/// The problems to show, by file in the order of the documents, errors first.
fn visible(problems: &[Problem], errors: bool, warnings: bool) -> Vec<Problem> {
    let mut shown: Vec<Problem> = problems
        .iter()
        .filter(|p| (p.severity == 1 && errors) || (p.severity == 2 && warnings) || (p.severity == 3 && warnings))
        .cloned()
        .collect();
    shown.sort_by(|a, b| a.path.cmp(&b.path).then(a.severity.cmp(&b.severity)).then(a.line.cmp(&b.line)).then(a.column.cmp(&b.column)));
    shown
}

impl BuiltinPlugin for ProblemsPlugin {
    fn core(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        "problems"
    }
    fn name(&self) -> &'static str {
        "Problèmes"
    }
    fn description(&self) -> &'static str {
        "Liste des erreurs et avertissements des documents ouverts, groupés par fichier, avec accès direct à la ligne."
    }
    fn icon(&self) -> &'static str {
        "problems"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }

    fn shortcuts(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        use egui::{Key, Modifiers};
        if ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::Num6)) {
            host.show_tool_window(&TOOL_WINDOWS[0]);
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let all = host.problems();
        let count = |severity: u8| all.iter().filter(|p| p.severity == severity).count();
        let (errors, warnings) = (count(1), count(2) + count(3));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (label, on, n, color) in [
                ("Erreurs", &mut self.errors, errors, Color32::from_rgb(0xF0, 0x56, 0x56)),
                ("Avertissements", &mut self.warnings, warnings, Color32::from_rgb(0xE6, 0xB5, 0x4A)),
            ] {
                let text = format!("{label} {n}");
                let galley = ui.painter().layout_no_wrap(text, FontId::proportional(12.5), if *on { t.text } else { t.text_secondary });
                let (rect, response) = ui.allocate_exact_size(egui::vec2(galley.size().x + 28.0, 24.0), egui::Sense::click());
                if *on {
                    ui.painter().rect_filled(rect, 5.0, t.hover);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 5.0, t.hover.gamma_multiply(0.6));
                }
                ui.painter().circle_filled(egui::pos2(rect.left() + 12.0, rect.center().y), 4.0, color);
                ui.painter().galley(egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, t.text);
                if response.clicked() {
                    *on = !*on;
                }
            }
        });
        let shown = visible(&all, self.errors, self.warnings);
        if shown.is_empty() {
            ui.add_space(12.0);
            let text = if all.is_empty() {
                "Aucun problème dans les documents ouverts"
            } else {
                "Aucun problème avec ces filtres"
            };
            ui.vertical_centered(|ui| ui.label(RichText::new(text).color(t.text_secondary)));
            return;
        }
        let mut rows: Vec<Row> = Vec::new();
        let mut index = 0;
        while index < shown.len() {
            let path = &shown[index].path;
            let end = shown[index..].iter().position(|p| &p.path != path).map_or(shown.len(), |n| index + n);
            rows.push(Row::File(path, end - index));
            if !self.collapsed.contains(path) {
                rows.extend(shown[index..end].iter().map(Row::Item));
            }
            index = end;
        }
        let root = host.root().to_path_buf();
        let mut open: Option<Problem> = None;
        let mut toggle: Option<PathBuf> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows[range] {
                match row {
                    Row::File(path, n) => {
                        let (rect, response) = ui::row(ui, false);
                        let collapsed = self.collapsed.contains(*path);
                        ui::paint_chevron(ui, egui::pos2(rect.left() + 12.0, rect.center().y), !collapsed);
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let end = ui::paint_text(ui, rect, rect.left() + 26.0, &name, t.text, ui::semibold(12.5));
                        let folder = path.strip_prefix(&root).ok().and_then(|p| p.parent()).map(|p| p.display().to_string()).unwrap_or_default();
                        let end = ui::paint_text(ui, rect, end + 8.0, &folder, t.text_secondary, FontId::proportional(12.0));
                        ui::paint_text(ui, rect, end + 8.0, &n.to_string(), t.text_secondary, FontId::proportional(12.0));
                        if response.clicked() {
                            toggle = Some((*path).clone());
                        }
                    }
                    Row::Item(problem) => {
                        let selected = self.selected.as_ref() == Some(&(problem.path.clone(), problem.line, problem.column));
                        let (rect, response) = ui::row(ui, selected);
                        let color = if problem.severity == 1 { Color32::from_rgb(0xF0, 0x56, 0x56) } else { Color32::from_rgb(0xE6, 0xB5, 0x4A) };
                        ui.painter().circle_filled(egui::pos2(rect.left() + 38.0, rect.center().y), 4.0, color);
                        let end = ui::paint_text(ui, rect, rect.left() + 50.0, &problem.message.lines().next().unwrap_or(""), t.text, FontId::proportional(12.5));
                        let place = format!("{}:{}", problem.line + 1, problem.column + 1);
                        let source = if problem.source.is_empty() { place } else { format!("{} · {place}", problem.source) };
                        ui::paint_text(ui, rect, end + 10.0, &source, t.text_secondary, FontId::proportional(12.0));
                        if response.clicked() {
                            self.selected = Some((problem.path.clone(), problem.line, problem.column));
                            open = Some((*problem).clone());
                        }
                    }
                }
            }
        });
        if let Some(path) = toggle
            && !self.collapsed.remove(&path)
        {
            self.collapsed.insert(path);
        }
        if let Some(problem) = open {
            host.open_at(problem.path, problem.line, problem.column, problem.length);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(path: &str, line: usize, severity: u8) -> Problem {
        Problem {
            path: PathBuf::from(path),
            line,
            column: 0,
            length: 1,
            severity,
            message: format!("problème {line}"),
            source: String::new(),
        }
    }

    #[test]
    fn problems_are_filtered_and_ordered_by_file_severity_and_line() {
        let all = vec![problem("b.js", 3, 2), problem("a.js", 9, 2), problem("a.js", 5, 1), problem("a.js", 1, 2), problem("c.js", 0, 4)];
        let shown = visible(&all, true, true);
        let order: Vec<(String, usize)> = shown.iter().map(|p| (p.path.display().to_string(), p.line)).collect();
        assert_eq!(order, [("a.js".to_string(), 5), ("a.js".to_string(), 1), ("a.js".to_string(), 9), ("b.js".to_string(), 3)], "errors first, hints left out");
        assert_eq!(visible(&all, true, false).len(), 1);
        assert_eq!(visible(&all, false, true).len(), 3);
    }
}
