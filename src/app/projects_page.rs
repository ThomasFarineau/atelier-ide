//! The Projects page, as IntelliJ's welcome screen lists them: every project opened, the most
//! recent first, to search, open here or in a new window, show in the file manager, copy the
//! path of or take off the list.

use super::{Ide, Pending, initials, project_color};
use crate::{project, ui};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, pos2, vec2,
};
use std::path::PathBuf;

const ROW_HEIGHT: f32 = 48.0;

/// When a project was last opened, as people say it.
fn opened(at: Option<u64>) -> String {
    let Some(at) = at else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let seconds = now.saturating_sub(at);
    match seconds {
        0..60 => tr!("projects.just_now").to_string(),
        60..3600 => trf!("projects.minutes_ago", count = seconds / 60),
        3600..86_400 => trf!("projects.hours_ago", count = seconds / 3600),
        _ => trf!("projects.days_ago", count = seconds / 86_400),
    }
}

enum Action {
    /// A click: this window or a new one, as the project widget asks.
    Open(PathBuf),
    Here(PathBuf),
    NewWindow(PathBuf),
    Reveal(PathBuf),
    Remove(PathBuf),
}

impl Ide {
    pub(super) fn open_projects_page(&mut self) {
        self.projects_filter.clear();
        self.show_projects = true;
    }

    pub(super) fn projects_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_projects {
            return;
        }
        let t = self.theme(ctx);
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() - 120.0).clamp(560.0, 820.0),
            (screen.height() - 120.0).clamp(420.0, 640.0),
        );
        let mut action = None;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("projects")).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.horizontal(|ui| {
                ui::dialog_title(ui, tr!("projects.title"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close |= ui::icon_button(ui, "close", tr!("close")).clicked();
                });
            });
            ui.horizontal(|ui| {
                let field = ui::search_field(
                    ui,
                    &mut self.projects_filter,
                    tr!("projects.search"),
                    (size.x - 340.0).max(200.0),
                );
                if self.projects_filter.is_empty() && !field.has_focus() {
                    field.request_focus();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::button(ui, tr!("projects.clone")).clicked() {
                        let clone = self
                            .builtins
                            .iter()
                            .filter(|p| self.builtin_enabled(p.id()))
                            .find(|p| p.commands().iter().any(|c| c.id == "git.clone"))
                            .map(|p| p.id());
                        if let Some(plugin) = clone {
                            self.run_builtin_command(plugin, "git.clone");
                            close = true;
                        }
                    }
                    if ui::primary_button(ui, tr!("projects.open")).clicked() {
                        self.choose_project(false);
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            let filter = self.projects_filter.trim().to_lowercase();
            let projects: Vec<PathBuf> = self
                .settings
                .recent
                .iter()
                .filter(|p| {
                    filter.is_empty() || p.to_string_lossy().to_lowercase().contains(&filter)
                })
                .cloned()
                .collect();
            if projects.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    let text = if self.settings.recent.is_empty() {
                        tr!("projects.none")
                    } else {
                        tr!("no_results")
                    };
                    ui.label(RichText::new(text).color(t.text_secondary));
                });
                return;
            }
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter && let Some(first) = projects.first() {
                action = Some(Action::Open(first.clone()));
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for path in projects {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        let exists = path.is_dir();
                        let current = project::simplify(&path) == project::simplify(&self.root);
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), ROW_HEIGHT),
                            Sense::click(),
                        );
                        ui::paint_row(ui, rect, current, response.hovered());
                        let color = if exists {
                            project_color(&name)
                        } else {
                            t.text_disabled
                        };
                        let badge = Rect::from_center_size(
                            pos2(rect.left() + 28.0, rect.center().y),
                            vec2(30.0, 30.0),
                        );
                        ui.painter().rect_filled(badge, 7.0, color);
                        ui.painter().text(
                            badge.center(),
                            Align2::CENTER_CENTER,
                            initials(&name),
                            ui::semibold(12.0),
                            Color32::WHITE,
                        );
                        let main = if exists { t.text } else { t.text_disabled };
                        let x = badge.right() + 12.0;
                        let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(0.0, 2.0)));
                        let title = if current {
                            format!("{name}  ·  {}", tr!("projects.current"))
                        } else if !exists {
                            format!("{name}  ·  {}", tr!("projects.missing"))
                        } else {
                            name.clone()
                        };
                        painter.text(
                            pos2(x, rect.top() + 8.0),
                            Align2::LEFT_TOP,
                            title,
                            ui::semibold(13.0),
                            main,
                        );
                        painter.text(
                            pos2(x, rect.top() + 27.0),
                            Align2::LEFT_TOP,
                            project::simplify(&path).display().to_string(),
                            FontId::proportional(11.5),
                            t.text_secondary,
                        );
                        painter.text(
                            pos2(rect.right() - 12.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            opened(self.settings.opened.get(&path).copied()),
                            FontId::proportional(11.5),
                            t.text_secondary,
                        );
                        if response.clicked() && exists && !current {
                            action = Some(Action::Open(path.clone()));
                        }
                        response.context_menu(|ui| {
                            ui::menu_begin(ui);
                            ui.add_enabled_ui(exists && !current, |ui| {
                                if ui::menu_item(
                                    ui,
                                    Some("openNewTab"),
                                    tr!("open_window"),
                                    None,
                                    false,
                                )
                                .clicked()
                                {
                                    action = Some(Action::Here(path.clone()));
                                    ui.close();
                                }
                            });
                            ui.add_enabled_ui(exists, |ui| {
                                if ui::menu_item(ui, None, tr!("open_new_window"), None, false)
                                    .clicked()
                                {
                                    action = Some(Action::NewWindow(path.clone()));
                                    ui.close();
                                }
                                if ui::menu_item(
                                    ui,
                                    Some("locate"),
                                    tr!("show_explorer"),
                                    None,
                                    false,
                                )
                                .clicked()
                                {
                                    action = Some(Action::Reveal(path.clone()));
                                    ui.close();
                                }
                            });
                            if ui::menu_item(ui, Some("copy"), tr!("copy_path"), None, false)
                                .clicked()
                            {
                                ui.ctx()
                                    .copy_text(project::simplify(&path).display().to_string());
                                ui.close();
                            }
                            ui::menu_separator(ui);
                            if ui::menu_item(
                                ui,
                                Some("delete"),
                                tr!("remove_from_list"),
                                None,
                                true,
                            )
                            .clicked()
                            {
                                action = Some(Action::Remove(path.clone()));
                                ui.close();
                            }
                        });
                    }
                });
        });
        if close || modal.should_close() {
            self.show_projects = false;
        }
        match action {
            Some(Action::Open(path)) => {
                self.show_projects = false;
                self.open_project(path);
            }
            Some(Action::Here(path)) => {
                self.show_projects = false;
                self.request(Pending::Project(path));
            }
            Some(Action::NewWindow(path)) => {
                self.show_projects = false;
                self.open_window(path);
            }
            Some(Action::Reveal(path)) => {
                let target = project::simplify(&path);
                #[cfg(windows)]
                let _ = std::process::Command::new("explorer").arg(&target).spawn();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&target).spawn();
                #[cfg(all(unix, not(target_os = "macos")))]
                let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
            }
            Some(Action::Remove(path)) => {
                self.settings.recent.retain(|p| p != &path);
                self.settings.opened.remove(&path);
            }
            None => {}
        }
    }
}
