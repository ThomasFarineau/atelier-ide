//! Spell checking in the editor: the misspelled words of the comments and strings are found in the
//! background and underlined in green; Alt+Enter on one offers the words that look like it and
//! to add it to the dictionary.

use super::{
    EditorState, Options,
    assist::{Choice, Chooser, Entry},
};
use crate::{
    languages::{QuickFix, TextEdit},
    spelling,
};
use eframe::egui::Ui;
use std::{
    ops::Range,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

/// The typing has to pause this long before the words are checked again.
const DELAY: Duration = Duration::from_millis(600);

#[derive(Default)]
pub(super) struct SpellState {
    /// The revision the typos are for.
    rev: Option<u64>,
    pub(super) typos: Vec<Range<usize>>,
    answer: Option<(Receiver<Vec<Range<usize>>>, u64)>,
}

impl EditorState {
    /// Keeps the typos up to date: asks for them once the typing has paused, takes the answer.
    pub(super) fn spell_frame(&mut self, ui: &Ui, text: &str, opt: &Options) {
        if !opt.spellcheck || !spelling::supports(opt.language) {
            self.spell = SpellState::default();
            return;
        }
        if let Some((rx, rev)) = &self.spell.answer {
            match rx.try_recv() {
                Ok(typos) => {
                    // The text moved on meanwhile: the typos were found for the text before.
                    let current = *rev == self.rev;
                    let rev = *rev;
                    self.spell.answer = None;
                    if current {
                        self.spell.typos = typos;
                        self.spell.rev = Some(rev);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(80));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.spell.answer = None,
            }
        }
        if self.spell.rev == Some(self.rev) || text.len() > spelling::MAX_TEXT {
            return;
        }
        if self.activity.elapsed() < DELAY && self.spell.rev.is_some() {
            ui.ctx().request_repaint_after(DELAY);
            return;
        }
        let Some(checker) = spelling::checker() else {
            // The dictionaries are being read.
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            return;
        };
        let (tx, rx) = mpsc::channel();
        let (snapshot, language) = (text.to_string(), opt.language.to_string());
        let repaint = ui.ctx().clone();
        std::thread::spawn(move || {
            let _ = tx.send(spelling::typos(&checker, &snapshot, &language));
            repaint.request_repaint();
        });
        self.spell.answer = Some((rx, self.rev));
    }

    /// The typo that holds `offset`.
    pub(super) fn typo_at(&self, offset: usize) -> Option<Range<usize>> {
        self.spell
            .typos
            .iter()
            .find(|r| r.start <= offset && offset <= r.end)
            .cloned()
    }

    /// What Alt+Enter offers on a typo: the words that look like it, and the dictionary.
    pub(super) fn spelling_entries(&self, text: &str) -> Vec<Entry> {
        let Some(range) = self.typo_at(self.cursor) else {
            return Vec::new();
        };
        let Some(checker) = spelling::checker() else {
            return Vec::new();
        };
        let word = text[range.clone()].to_string();
        let mut entries: Vec<Entry> = checker
            .suggestions(&word)
            .into_iter()
            .map(|suggestion| {
                let title = trf!("spelling_replace", word = suggestion);
                Entry {
                    label: title.clone(),
                    detail: String::new(),
                    kind: None,
                    indent: 0,
                    choice: Choice::Fix(QuickFix {
                        title,
                        edits: vec![TextEdit {
                            range: range.clone(),
                            new_text: suggestion,
                        }],
                    }),
                }
            })
            .collect();
        entries.push(Entry {
            label: trf!("spelling_add", word = word),
            detail: String::new(),
            kind: None,
            indent: 0,
            choice: Choice::Dictionary(word),
        });
        entries
    }

    /// Alt+Enter in a file without a language service: the spelling choices alone.
    pub(super) fn spell_fixes_alone(&mut self, text: &str, opt: &Options) {
        if opt.assist.is_some() || !std::mem::take(&mut self.assist.want_fixes) {
            return;
        }
        let entries = self.spelling_entries(text);
        if !entries.is_empty() {
            self.assist.chooser = Some(Chooser::new(tr!("quick_fixes"), entries, true));
        }
    }

    /// The word was added to the dictionary: it is no typo any more, here and in the next check.
    pub(super) fn word_added(&mut self, text: &str, word: &str) {
        if let Some(checker) = spelling::checker() {
            checker.add(word);
        }
        let lower = word.to_lowercase();
        self.spell.typos.retain(|r| {
            text.get(r.clone())
                .is_none_or(|w| w.to_lowercase() != lower)
        });
    }
}
