//! The find and replace bar of the editor, as IntelliJ's: case, whole word and regular
//! expression options, the position among the matches, replace and replace all.

use crate::{
    search::{self, Options},
    ui::{self, Theme},
};
use eframe::egui::{self, Align, Key, Layout, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use regex::Regex;
use std::ops::Range;

const MAX_MATCHES: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Next,
    Previous,
    Replace,
    ReplaceAll,
    Close,
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub options: Options,
    pub show_replace: bool,
    regex: Option<Regex>,
    error: Option<String>,
    matches: Vec<Range<usize>>,
    /// What the matches were computed for: document, its revision, the query and options.
    key: Option<(usize, u64, String, Options)>,
    focus: bool,
}

impl Find {
    /// Shows the bar. A selection on one line becomes the query.
    pub fn show(&mut self, selection: Option<String>, with_replace: bool) {
        self.open = true;
        self.focus = true;
        if with_replace {
            self.show_replace = true;
        }
        if let Some(text) = selection.filter(|s| !s.is_empty() && !s.contains('\n') && s.len() < 200) {
            self.query = text;
        }
    }

    pub fn matches(&self) -> &[Range<usize>] {
        &self.matches
    }

    pub fn regex(&self) -> Option<&Regex> {
        self.regex.as_ref()
    }

    /// Looks for the matches again when the document, the query or the options changed.
    pub fn refresh(&mut self, document: usize, revision: u64, text: &str) {
        let key = (document, revision, self.query.clone(), self.options);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.matches.clear();
        self.error = None;
        self.regex = None;
        if self.query.is_empty() {
            return;
        }
        match search::build(&self.query, self.options) {
            Ok(re) => {
                self.matches = search::find_all(text, &re, MAX_MATCHES);
                self.regex = Some(re);
            }
            Err(message) => self.error = Some(message),
        }
    }

    /// Which match the selection is, 0-based.
    pub fn position(&self, selection: &Range<usize>) -> Option<usize> {
        self.matches.iter().position(|m| m == selection)
    }

    /// The match after the selection (wrapping), or before it.
    pub fn neighbor(&self, selection: &Range<usize>, forward: bool) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        if forward {
            self.matches
                .iter()
                .find(|m| m.start >= selection.end)
                .or_else(|| self.matches.first())
                .cloned()
        } else {
            self.matches
                .iter()
                .rev()
                .find(|m| m.end <= selection.start)
                .or_else(|| self.matches.last())
                .cloned()
        }
    }

    /// Draws the bar. The action asked for, if any.
    pub fn ui(&mut self, ui: &mut Ui, t: &Theme, selection: &Range<usize>) -> Action {
        let mut action = Action::None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let chevron = if self.show_replace { "chevronDown" } else { "chevronRight" };
            if ui::icon_toggle(ui, chevron, self.show_replace, "Remplacer · Ctrl+R").clicked() {
                self.show_replace = !self.show_replace;
            }
            let field = ui::search_field(ui, &mut self.query, "Rechercher", 280.0);
            if self.focus {
                field.request_focus();
                self.focus = false;
            }
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            if (field.has_focus() || field.lost_focus()) && enter {
                action = if ui.input(|i| i.modifiers.shift) { Action::Previous } else { Action::Next };
                field.request_focus();
            }
            // Alt+C, Alt+W, Alt+X as in IntelliJ.
            let alt = |key| ui.input(|i| i.modifiers.alt && i.key_pressed(key));
            let (case, word, regex) = (alt(Key::C), alt(Key::W), alt(Key::X));
            self.options.case ^= toggle(ui, t, "Aa", self.options.case, "Respecter la casse · Alt+C") | case;
            self.options.word ^= toggle(ui, t, "W", self.options.word, "Mots entiers · Alt+W") | word;
            self.options.regex ^= toggle(ui, t, ".*", self.options.regex, "Expression régulière · Alt+X") | regex;
            let count = self.matches.len();
            let (text, color) = if let Some(error) = &self.error {
                (error.clone(), t.error)
            } else if self.query.is_empty() {
                (String::new(), t.text_secondary)
            } else if count == 0 {
                ("Aucun résultat".to_string(), t.error)
            } else {
                let shown = if count >= MAX_MATCHES { format!("{count}+") } else { count.to_string() };
                match self.position(selection) {
                    Some(i) => (format!("{} sur {shown}", i + 1), t.text_secondary),
                    None => (format!("{shown} résultats"), t.text_secondary),
                }
            };
            ui.label(RichText::new(text).size(12.5).color(color));
            if ui::icon_button(ui, "up", "Occurrence précédente · Maj+Entrée").clicked() {
                action = Action::Previous;
            }
            if ui::icon_button(ui, "down", "Occurrence suivante · Entrée").clicked() {
                action = Action::Next;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::icon_button(ui, "close", "Fermer · Échap").clicked() {
                    action = Action::Close;
                }
            });
        });
        if self.show_replace {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(ui::BUTTON + 4.0);
                let field = ui::search_field(ui, &mut self.replace, "Remplacer par", 280.0);
                if field.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    action = Action::Replace;
                }
                let usable = self.regex.is_some() && !self.matches.is_empty();
                ui.add_enabled_ui(usable, |ui| {
                    if ui::button(ui, "Remplacer").clicked() {
                        action = Action::Replace;
                    }
                    if ui::button(ui, "Tout remplacer").clicked() {
                        action = Action::ReplaceAll;
                    }
                });
            });
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            action = Action::Close;
        }
        action
    }
}

/// A small text toggle. True when it was clicked.
fn toggle(ui: &mut Ui, t: &Theme, label: &str, on: bool, tooltip: &str) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), ui::semibold(12.0), if on { egui::Color32::WHITE } else { t.text_secondary });
    let (rect, response) = ui.allocate_exact_size(vec2((galley.size().x + 12.0).max(26.0), 24.0), Sense::click());
    if on {
        ui.painter().rect_filled(rect, 5.0, t.accent);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.hover);
    } else {
        ui.painter().rect_stroke(rect, 5.0, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, t.text);
    response.on_hover_text(tooltip).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_follow_the_query_the_options_and_the_text() {
        let mut find = Find::default();
        find.query = "cat".into();
        find.refresh(0, 1, "Cat cat concat");
        assert_eq!(find.matches().len(), 3);
        find.options.word = true;
        find.refresh(0, 1, "Cat cat concat");
        assert_eq!(find.matches().len(), 2);
        // The same key is not computed again; a new revision is.
        find.refresh(0, 2, "cat");
        assert_eq!(find.matches().len(), 1);
        find.query = "(".into();
        find.options.regex = true;
        find.refresh(0, 2, "cat");
        assert!(find.matches().is_empty() && find.error.is_some());
    }

    #[test]
    fn next_and_previous_wrap_around_the_selection() {
        let mut find = Find::default();
        find.query = "a".into();
        find.refresh(0, 1, "a_a_a");
        assert_eq!(find.matches(), [0..1, 2..3, 4..5]);
        assert_eq!(find.neighbor(&(0..1), true), Some(2..3));
        assert_eq!(find.neighbor(&(4..5), true), Some(0..1), "wraps");
        assert_eq!(find.neighbor(&(1..1), true), Some(2..3), "from a caret");
        assert_eq!(find.neighbor(&(2..3), false), Some(0..1));
        assert_eq!(find.neighbor(&(0..1), false), Some(4..5), "wraps backwards");
        assert_eq!(find.position(&(2..3)), Some(1));
    }
}
