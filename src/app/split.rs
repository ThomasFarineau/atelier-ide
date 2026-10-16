//! Split Right and Split Down: a second editor beside or under the first. It shows the file that was active when it was
//! opened, with a view of its own (caret, scroll, folds): the text is the document's, so what is
//! typed in one editor shows in the other. Another file can be put there by making it active and
//! splitting again.

use super::Ide;
use crate::{
    editor::{Assist, EditorState, Options},
    languages::{DocRef, Service},
    project::Document,
    ui,
};
use eframe::egui::{self, FontId, Margin, Sense, Stroke, pos2, vec2};
use std::path::PathBuf;

pub struct SplitPane {
    /// The file shown; the pane goes away when it is closed.
    pub path: PathBuf,
    pub view: EditorState,
    /// Under the first editor rather than beside it.
    pub down: bool,
    /// The revision of the document's own view the last time this one was drawn: a change means
    /// the text moved under it.
    seen: u64,
}

impl Ide {
    /// Split Right (or Down): the active file in a second editor.
    pub(super) fn split_to(&mut self, down: bool) {
        let Some(doc) = self.documents.get(self.active) else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            self.status = tr!("split_needs_file").into();
            return;
        };
        let mut view = EditorState::default();
        view.select(doc.view.cursor, doc.view.cursor);
        self.split = Some(SplitPane {
            path,
            view,
            down,
            seen: u64::MAX,
        });
    }

    /// The index of the document the split shows when it is not the active one. Closes the split
    /// when its file is no longer open.
    pub(super) fn split_other(&mut self) -> Option<usize> {
        let path = self.split.as_ref()?.path.clone();
        let Some(index) = self
            .documents
            .iter()
            .position(|d| d.path.as_ref() == Some(&path))
        else {
            self.split = None;
            return None;
        };
        (index != self.active).then_some(index)
    }
}

/// Draws the pane in `ui`: a bar with the name of the file, then the editor. `service` is given
/// when the file is not the one of the first editor (the language service follows one view of a
/// file). Whether the pane asked to be closed.
pub fn show(
    ui: &mut egui::Ui,
    pane: &mut SplitPane,
    doc: &mut Document,
    service: Option<&Service>,
    settings: &super::Settings,
    theme: ui::Theme,
    font: FontId,
) -> bool {
    let mut close = false;
    // The bar.
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let painter = ui.painter_at(bar);
    painter.rect_filled(bar, 0.0, theme.island);
    painter.line_segment(
        [bar.left_bottom(), bar.right_bottom()],
        Stroke::new(1.0_f32, theme.separator),
    );
    painter.text(
        pos2(bar.left() + 10.0, bar.center().y),
        egui::Align2::LEFT_CENTER,
        doc.title(),
        FontId::proportional(12.5),
        theme.text,
    );
    let button = egui::Rect::from_min_size(pos2(bar.right() - 24.0, bar.top()), vec2(24.0, 24.0));
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(button));
    if ui::icon_button(&mut bar_ui, "closeSmall", tr!("close")).clicked() {
        close = true;
    }
    if doc.loading.is_some() || doc.binary {
        return close;
    }
    // The text moved under this view: its index is read again.
    if pane.seen != doc.view.revision() {
        pane.view.invalidate();
    }
    let assist = service
        .filter(|s| s.formats(&doc.language))
        .map(|service| Assist {
            service,
            doc: DocRef {
                key: super::doc_key(&doc.path, usize::MAX),
                path: doc.path.clone(),
                language: doc.language.clone(),
            },
        });
    let has_assist = assist.is_some();
    let options = Options {
        language: &doc.language,
        font,
        theme,
        marks: &[],
        annotations: None,
        gutter_commands: &[],
        assist,
        search: &[],
        read_only: doc.read_only,
        inlay_hints: has_assist && !settings.inlay_hints_off,
        sticky_lines: !settings.sticky_lines_off,
        breadcrumbs: has_assist && !settings.breadcrumbs_off,
        bookmarks: &[],
        spellcheck: !settings.spellcheck_off,
    };
    let id = egui::Id::new(("split_source", doc.path.clone()));
    egui::Frame::new()
        .inner_margin(Margin::ZERO)
        .show(ui, |ui| {
            let output = pane.view.show(ui, id, &mut doc.text, &options);
            if output.changed {
                // The other editor reads the text again too.
                doc.view.invalidate();
            }
        });
    pane.seen = doc.view.revision();
    close
}
