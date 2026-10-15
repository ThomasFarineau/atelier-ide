//! Recent Locations (Ctrl+Shift+E) and the Switcher (Ctrl+Tab): the places and files used last.

use super::{Hit, Ide, NavPoint, PaletteKey, PaletteMode, Target, bookmarks, history};
use crate::ui;
use eframe::egui::{self, Align2, FontId, Key, Margin, RichText, pos2, vec2};
use std::{fs, path::Path};

/// Places shown by Recent Locations at most.
const LOCATIONS: usize = 60;
/// Files bigger than this are not read to show the line of a place.
const PREVIEW_LIMIT: u64 = 1024 * 1024;

/// The 0-based line holding `offset` in `text`, and that line trimmed.
fn line_at(text: &str, offset: usize) -> (usize, String) {
    let offset = offset.min(text.len());
    let line = text.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count();
    let preview = text.lines().nth(line).unwrap_or("").trim().to_string();
    (line, preview)
}

impl Ide {
    /// The text of a file for a preview: the open document, else the file when it is small.
    fn preview_text(&self, path: &Path) -> Option<String> {
        if let Some(doc) = self
            .documents
            .iter()
            .find(|d| d.path.as_deref() == Some(path))
            && doc.loading.is_none()
        {
            return Some(doc.text.clone());
        }
        let size = fs::metadata(path).ok()?.len();
        (size <= PREVIEW_LIMIT)
            .then(|| fs::read_to_string(path).ok())
            .flatten()
    }

    /// Recent Locations: the places left by jumps and the place of the caret, the latest first,
    /// each with its line.
    pub(super) fn location_hits(&mut self) -> Vec<Hit> {
        let query = self.palette_query.trim().to_lowercase();
        let key = PaletteKey {
            mode: PaletteMode::Locations,
            query: query.clone(),
            version: (self.nav_back.len(), self.nav_forward.len() as u64),
        };
        if let Some((kept, hits)) = &self.palette_cache
            && *kept == key
        {
            return hits.clone();
        }
        let here = self.documents.get(self.active).and_then(|d| {
            Some(NavPoint {
                path: d.path.clone()?,
                offset: d.view.cursor,
            })
        });
        let points: Vec<NavPoint> = here
            .into_iter()
            .chain(self.nav_back.iter().rev().cloned())
            .chain(self.nav_forward.iter().rev().cloned())
            .collect();
        let mut hits: Vec<Hit> = Vec::new();
        let mut seen: Vec<(std::path::PathBuf, usize)> = Vec::new();
        for point in points {
            if hits.len() >= LOCATIONS {
                break;
            }
            let Some(text) = self.preview_text(&point.path) else {
                continue;
            };
            let (line, preview) = line_at(&text, point.offset);
            if seen.contains(&(point.path.clone(), line)) {
                continue;
            }
            seen.push((point.path.clone(), line));
            let name = point
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let label = if preview.is_empty() {
                name.clone()
            } else {
                preview
            };
            if !query.is_empty()
                && !label.to_lowercase().contains(&query)
                && !name.to_lowercase().contains(&query)
            {
                continue;
            }
            hits.push(Hit {
                label,
                hint: format!("{name}:{}", line + 1),
                icon: None,
                file: Some(point.path.clone()),
                target: Target::Place(point.path, line),
            });
        }
        self.palette_cache = Some((key, hits.clone()));
        hits
    }

    /// Ctrl+Tab and Ctrl+Shift+Tab: opens the switcher on the previous file, or goes on down (or
    /// up) its list.
    pub(super) fn switch_key(&mut self, forward: bool) {
        let count = self.switcher_files().len();
        if count == 0 {
            return;
        }
        let next = match self.switcher {
            None if forward => 1.min(count - 1),
            None => count - 1,
            Some(i) if forward => (i + 1) % count,
            Some(i) => (i + count - 1) % count,
        };
        self.switcher = Some(next);
    }

    /// The files the switcher lists: the ones used last, still on disk.
    fn switcher_files(&self) -> Vec<std::path::PathBuf> {
        self.recent_files
            .iter()
            .filter(|p| p.is_file())
            .take(20)
            .cloned()
            .collect()
    }

    /// The switcher popup: it follows Ctrl, and opens the chosen file once Ctrl is released.
    pub(super) fn switcher(&mut self, ctx: &egui::Context) {
        let Some(mut index) = self.switcher else {
            return;
        };
        let files = self.switcher_files();
        if files.is_empty() {
            self.switcher = None;
            return;
        }
        let (down, up, escape, held) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
                i.modifiers.command,
            )
        });
        if down {
            index = (index + 1) % files.len();
        }
        if up {
            index = (index + files.len() - 1) % files.len();
        }
        index = index.min(files.len() - 1);
        self.switcher = Some(index);
        if escape {
            self.switcher = None;
            return;
        }
        if !held {
            self.switcher = None;
            self.open_file(files[index].clone());
            return;
        }
        let t = self.theme(ctx);
        let width = 460.0_f32.min(ctx.content_rect().width() - 40.0);
        egui::Area::new(egui::Id::new("switcher"))
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(t.popup)
                    .corner_radius(10)
                    .inner_margin(Margin::symmetric(0, 8))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            ui.label(RichText::new(tr!("recent_files")).font(ui::semibold(13.0)));
                        });
                        ui.add_space(4.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, path) in files.iter().enumerate() {
                            let (rect, response) = ui::row(ui, i == index);
                            let icon = egui::Rect::from_min_size(
                                pos2(rect.left() + 16.0, rect.center().y - 8.0),
                                vec2(16.0, 16.0),
                            );
                            egui::Image::new(self.file_icon(ui.ctx(), path)).paint_at(ui, icon);
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            ui::paint_text(
                                ui,
                                rect,
                                icon.right() + 8.0,
                                &name,
                                t.text,
                                FontId::proportional(13.0),
                            );
                            let folder = path
                                .parent()
                                .map(|p| {
                                    p.strip_prefix(&self.root)
                                        .unwrap_or(p)
                                        .display()
                                        .to_string()
                                })
                                .unwrap_or_default();
                            ui.painter().with_clip_rect(rect).text(
                                pos2(rect.right() - 16.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                folder,
                                FontId::proportional(12.0),
                                t.text_secondary,
                            );
                            if response.clicked() {
                                self.switcher = Some(i);
                            }
                        }
                    });
            });
    }

    // ---- Bookmarks ----------------------------------------------------------------------

    /// F11: a bookmark on the line of the caret, or none any more.
    pub(super) fn toggle_bookmark(&mut self) {
        let Some(doc) = self.documents.get(self.active) else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            return;
        };
        let line = doc.view.position(&doc.text).0 - 1;
        let text = doc.text.lines().nth(line).unwrap_or("").to_string();
        let set = bookmarks::toggle(&mut self.bookmarks, &path, line, &text);
        bookmarks::save(&crate::cli::data_dir(), &self.root, &self.bookmarks);
        self.status = if set {
            tr!("bookmark_set").into()
        } else {
            tr!("bookmark_removed").into()
        };
    }

    /// The lines of a document that have a bookmark; the bookmarks follow their line first when
    /// the text changed. Takes the fields it needs, so that the document can stay borrowed.
    pub(super) fn bookmark_lines(
        list: &mut [bookmarks::Bookmark],
        seen: &mut (usize, u64),
        active: usize,
        root: &Path,
        path: &Option<std::path::PathBuf>,
        text: &str,
        revision: u64,
    ) -> Vec<usize> {
        let Some(path) = path else {
            return Vec::new();
        };
        if list.iter().all(|b| &b.path != path) {
            return Vec::new();
        }
        if *seen != (active, revision) {
            *seen = (active, revision);
            let lines: Vec<&str> = text.lines().collect();
            if bookmarks::follow(list, path, &lines) {
                bookmarks::save(&crate::cli::data_dir(), root, list);
            }
        }
        list.iter()
            .filter(|b| &b.path == path)
            .map(|b| b.line)
            .collect()
    }

    /// Shift+F11: the bookmarks, by file and line.
    pub(super) fn bookmark_hits(&self, query: &str) -> Vec<Hit> {
        let mut found: Vec<&bookmarks::Bookmark> = self.bookmarks.iter().collect();
        found.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        found
            .into_iter()
            .filter(|b| {
                query.is_empty()
                    || b.text.to_lowercase().contains(query)
                    || b.path.to_string_lossy().to_lowercase().contains(query)
            })
            .map(|b| {
                let name = b
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                Hit {
                    label: if b.text.is_empty() { name.clone() } else { b.text.clone() },
                    hint: format!("{name}:{}", b.line + 1),
                    icon: None,
                    file: Some(b.path.clone()),
                    target: Target::Place(b.path.clone(), b.line),
                }
            })
            .collect()
    }

    // ---- Local History ------------------------------------------------------------------

    /// The saved versions of the active file, the latest first, each with what it changes
    /// against the text now.
    pub(super) fn history_hits(&mut self) -> Vec<Hit> {
        let Some(doc) = self.documents.get(self.active) else {
            return Vec::new();
        };
        let Some(path) = doc.path.clone() else {
            return Vec::new();
        };
        let key = PaletteKey {
            mode: PaletteMode::History,
            query: String::new(),
            version: (self.active, doc.view.revision()),
        };
        if let Some((kept, hits)) = &self.palette_cache
            && *kept == key
        {
            return hits.clone();
        }
        let current = doc.text.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let hits: Vec<Hit> = history::list(&crate::cli::data_dir(), &self.root, &path)
            .into_iter()
            .map(|version| {
                let (added, removed) = std::fs::read_to_string(&version.path)
                    .map(|old| history::changes(&current, &old))
                    .unwrap_or((0, 0));
                let seconds = now.saturating_sub(version.at) / 1000;
                let when = match seconds {
                    0..60 => tr!("just_now").into(),
                    60..3600 => trf!("min_ago", s = seconds / 60),
                    3600..86_400 => trf!("h_ago", s = seconds / 3600),
                    _ => trf!("days_ago", s = seconds / 86_400),
                };
                Hit {
                    label: when,
                    hint: format!("+{added} −{removed}  {} B", version.size),
                    icon: None,
                    file: Some(path.clone()),
                    target: Target::Version(version.path),
                }
            })
            .collect();
        self.palette_cache = Some((key, hits.clone()));
        hits
    }

    /// Puts a saved version in the active document, as an edit that can be undone.
    pub(super) fn restore_version(&mut self, version: &Path) {
        let Ok(text) = std::fs::read_to_string(version) else {
            self.status = tr!("version_unreadable").into();
            return;
        };
        if let Some(doc) = self.documents.get_mut(self.active)
            && !doc.read_only
        {
            doc.view.replace_text(&mut doc.text, &text);
            self.status = tr!("version_restored").into();
        }
    }
}
