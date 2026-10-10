//! Reformat Code (Ctrl+Alt+L), Optimize Imports (Ctrl+Alt+O) and the Reformat File dialog
//! (Ctrl+Alt+Shift+L), as IntelliJ has them: the whole file, the selected text or only the
//! lines changed since the last commit, with the imports optimized first when asked.

use super::Ide;
use crate::{editor::FormatRequest, languages::format, plugins::builtin::MarkKind, ui};
use eframe::egui;

/// What the dialog formats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Scope {
    #[default]
    File,
    Selection,
    Changed,
}

/// The choices of the dialog, kept for the next time as IntelliJ does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Choice {
    pub scope: Scope,
    pub organize_imports: bool,
}

impl Ide {
    pub(super) fn reformat(&mut self, request: FormatRequest) {
        match self.documents.get_mut(self.active) {
            Some(doc) => doc.view.request_format(request),
            None => self.status = tr!("no_file_open").into(),
        }
    }

    /// Ctrl+Alt+L: the selection when there is one, else the whole file.
    pub(super) fn reformat_code(&mut self) {
        let Some(doc) = self.documents.get(self.active) else {
            return;
        };
        let selection = doc.view.selection();
        let lines = (!selection.is_empty()).then(|| vec![format::lines_of(&doc.text, selection)]);
        self.reformat(FormatRequest {
            lines,
            ..Default::default()
        });
    }

    pub(super) fn open_reformat_dialog(&mut self) {
        if self.documents.get(self.active).is_none() {
            return;
        }
        let mut choice = self.reformat_choice;
        // The selection is the choice when there is one, as in IntelliJ.
        let has_selection = self
            .documents
            .get(self.active)
            .is_some_and(|d| !d.view.selection().is_empty());
        if has_selection {
            choice.scope = Scope::Selection;
        } else if choice.scope == Scope::Selection {
            choice.scope = Scope::File;
        }
        self.reformat_dialog = Some(choice);
    }

    /// The lines of the active document changed since the last commit, from the change marks of
    /// the plugins (Git).
    fn changed_lines(&mut self) -> Vec<std::ops::Range<usize>> {
        let Some((path, text)) = self
            .documents
            .get(self.active)
            .and_then(|d| Some((d.path.clone()?, d.text.clone())))
        else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        self.each_builtin(|plugin, _| {
            for mark in plugin.line_marks(&path, &text) {
                if matches!(mark.kind, MarkKind::Added | MarkKind::Modified) {
                    lines.push(mark.line..mark.line + 1);
                }
            }
        });
        lines
    }

    pub(super) fn reformat_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut choice) = self.reformat_dialog else {
            return;
        };
        let Some(doc) = self.documents.get(self.active) else {
            self.reformat_dialog = None;
            return;
        };
        let title = trf!("reformat_file_2", doc = doc.title());
        let has_selection = !doc.view.selection().is_empty();
        let has_path = doc.path.is_some();
        let modal = egui::Modal::new(egui::Id::new("reformat")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui::dialog_title(ui, &title);
            ui::section(ui, tr!("options"));
            ui::checkbox(ui, &mut choice.organize_imports, tr!("optimize_imports_2"));
            ui.add_space(8.0);
            ui::section(ui, tr!("scope"));
            ui::radio_value(ui, &mut choice.scope, Scope::File, tr!("whole_file"));
            ui.add_enabled_ui(has_selection, |ui| {
                ui::radio_value(
                    ui,
                    &mut choice.scope,
                    Scope::Selection,
                    tr!("selected_text"),
                );
            });
            ui.add_enabled_ui(has_path, |ui| {
                ui::radio_value(
                    ui,
                    &mut choice.scope,
                    Scope::Changed,
                    tr!("only_vcs_changed_text"),
                );
            });
            ui::dialog_buttons(ui, tr!("run"))
        });
        let (ok, cancel) = modal.inner;
        if cancel || modal.should_close() {
            self.reformat_dialog = None;
            return;
        }
        self.reformat_dialog = Some(choice);
        if !ok {
            return;
        }
        self.reformat_dialog = None;
        self.reformat_choice = choice;
        let lines = match choice.scope {
            Scope::File => None,
            Scope::Selection => self
                .documents
                .get(self.active)
                .map(|d| vec![format::lines_of(&d.text, d.view.selection())]),
            Scope::Changed => {
                let lines = self.changed_lines();
                if lines.is_empty() {
                    self.status = tr!("no_lines_changed_since_last").into();
                    return;
                }
                Some(lines)
            }
        };
        self.reformat(FormatRequest {
            organize_imports: choice.organize_imports,
            lines,
            imports_only: false,
        });
    }
}
