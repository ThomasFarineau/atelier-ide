//! The Projects page, as IntelliJ's welcome screen lists them: every project opened, the most
//! recent first, to search, open here or in a new window, show in the file manager, copy the
//! path of or take off the list.

use super::{Ide, Pending, TITLE_HEIGHT, initials, project_color};
use crate::{project, ui};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, pos2, vec2,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

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

/// The project the New Project dialog creates: `location/name`.
pub(super) struct NewProject {
    name: String,
    location: String,
    error: Option<String>,
}

/// Whether `name` can be the name of a folder, on any system.
fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !name.chars().any(|c| {
            matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
        })
}

/// Creates the folder of a new project, `location/name`, which must not hold anything yet.
pub(super) fn create_project(location: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(tr!("projects.name_invalid").into());
    }
    let path = location.join(name.trim());
    let taken = if path.is_dir() {
        fs::read_dir(&path).is_ok_and(|mut entries| entries.next().is_some())
    } else {
        path.exists()
    };
    if taken {
        return Err(tr!("projects.exists").into());
    }
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
}

impl Ide {
    pub(super) fn open_projects_page(&mut self) {
        self.projects_filter.clear();
        self.show_projects = true;
    }

    /// Where folders are chosen from: the project, or next to the last one when the window
    /// has none.
    pub(super) fn projects_folder(&self) -> PathBuf {
        if !self.start_screen {
            return self.root.clone();
        }
        self.settings
            .recent
            .iter()
            .filter(|p| p.is_dir())
            .find_map(|p| p.parent().map(project::simplify))
            .unwrap_or_else(home)
    }

    fn open_new_project(&mut self) {
        let location = if self.start_screen {
            self.projects_folder()
        } else {
            self.root
                .parent()
                .map(project::simplify)
                .unwrap_or_else(home)
        };
        self.new_project = Some(NewProject {
            name: String::new(),
            location: location.display().to_string(),
            error: None,
        });
    }

    /// The New Project dialog: a name, a folder, and the project opens.
    pub(super) fn new_project_dialog(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.new_project else {
            return;
        };
        let t = ui::theme(ctx);
        let mut browse = false;
        let modal = egui::Modal::new(egui::Id::new("new_project")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui::dialog_title(ui, tr!("projects.new_title"));
            egui::Grid::new("new_project_grid")
                .num_columns(2)
                .spacing(vec2(10.0, 8.0))
                .show(ui, |ui| {
                    ui.label(tr!("projects.name"));
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .hint_text(tr!("projects.name_hint"))
                            .desired_width(360.0),
                    );
                    if draft.name.is_empty() && !field.has_focus() {
                        field.request_focus();
                    }
                    if field.changed() {
                        draft.error = None;
                    }
                    ui.end_row();
                    ui.label(tr!("projects.location"));
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut draft.location).desired_width(262.0),
                        );
                        if field.changed() {
                            draft.error = None;
                        }
                        browse = ui::button(ui, tr!("projects.browse")).clicked();
                    });
                    ui.end_row();
                });
            let target = Path::new(draft.location.trim()).join(draft.name.trim());
            let line = match &draft.error {
                Some(error) => RichText::new(error.as_str()).color(t.error),
                None if draft.name.trim().is_empty() => RichText::new(" "),
                None => RichText::new(trf!("projects.will_create", path = target.display()))
                    .color(t.text_secondary),
            };
            ui.add_space(4.0);
            ui.label(line.small());
            ui::dialog_buttons(ui, tr!("create"))
        });
        let (ok, cancel) = modal.inner;
        if browse
            && let Some(folder) = rfd::FileDialog::new()
                .set_directory(draft.location.trim())
                .pick_folder()
        {
            draft.location = project::simplify(&folder).display().to_string();
            draft.error = None;
        }
        if cancel || modal.should_close() {
            self.new_project = None;
        } else if ok {
            match create_project(Path::new(draft.location.trim()), &draft.name) {
                Ok(path) => {
                    self.new_project = None;
                    self.show_projects = false;
                    self.open_project(path);
                }
                Err(error) => draft.error = Some(error),
            }
        }
    }

    /// The window started without a project: its own title bar and the Projects page.
    pub(super) fn start_screen_ui(&mut self, ctx: &egui::Context) {
        let t = self.theme(ctx);
        egui::TopBottomPanel::top("start_title")
            .exact_height(TITLE_HEIGHT)
            .frame(egui::Frame::new().fill(t.frame))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let bar = ui.interact(
                    ui.max_rect(),
                    egui::Id::new("start_title_bar"),
                    Sense::click_and_drag(),
                );
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if bar.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if bar.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new("Atelier IDE").font(ui::semibold(13.0)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.window_controls(ui, maximized);
                    });
                });
            });
        let mut action = None;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.island)
                    .inner_margin(Margin::symmetric(28, 20)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new(tr!("projects.welcome")).font(ui::semibold(20.0)));
                ui.add_space(14.0);
                let width = ui.available_width();
                self.projects_body(ui, width, &mut action, None);
            });
        self.projects_action(action);
    }

    pub(super) fn projects_dialog(&mut self, ctx: &egui::Context) {
        // The start screen shows the page in the window itself.
        if !self.show_projects || self.start_screen {
            return;
        }
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
            self.projects_body(ui, size.x, &mut action, Some(&mut close));
        });
        if close || modal.should_close() {
            self.show_projects = false;
        }
        self.projects_action(action);
    }

    /// The page: its title with a close button (when `close` is given), the search, the
    /// buttons and the list of the projects.
    fn projects_body(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        action: &mut Option<Action>,
        close: Option<&mut bool>,
    ) {
        let t = self.theme(ui.ctx());
        if let Some(close) = close {
            ui.horizontal(|ui| {
                ui::dialog_title(ui, tr!("projects.title"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    *close |= ui::icon_button(ui, "close", tr!("close")).clicked();
                });
            });
        }
        let mut close = false;
        ui.horizontal(|ui| {
            let field = ui::search_field(
                ui,
                &mut self.projects_filter,
                tr!("projects.search"),
                (width - 480.0).max(200.0),
            );
            if self.projects_filter.is_empty() && !field.has_focus() && self.new_project.is_none() {
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
                if ui::button(ui, tr!("projects.new")).clicked() {
                    self.open_new_project();
                }
            });
        });
        if close {
            self.show_projects = false;
        }
        ui.add_space(8.0);
        let filter = self.projects_filter.trim().to_lowercase();
        let projects: Vec<PathBuf> = self
            .settings
            .recent
            .iter()
            .filter(|p| filter.is_empty() || p.to_string_lossy().to_lowercase().contains(&filter))
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
        if enter
            && self.new_project.is_none()
            && let Some(first) = projects.first()
        {
            *action = Some(Action::Open(first.clone()));
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
                        *action = Some(Action::Open(path.clone()));
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
                                *action = Some(Action::Here(path.clone()));
                                ui.close();
                            }
                        });
                        ui.add_enabled_ui(exists, |ui| {
                            if ui::menu_item(ui, None, tr!("open_new_window"), None, false)
                                .clicked()
                            {
                                *action = Some(Action::NewWindow(path.clone()));
                                ui.close();
                            }
                            if ui::menu_item(ui, Some("locate"), tr!("show_explorer"), None, false)
                                .clicked()
                            {
                                *action = Some(Action::Reveal(path.clone()));
                                ui.close();
                            }
                        });
                        if ui::menu_item(ui, Some("copy"), tr!("copy_path"), None, false).clicked()
                        {
                            ui.ctx()
                                .copy_text(project::simplify(&path).display().to_string());
                            ui.close();
                        }
                        ui::menu_separator(ui);
                        if ui::menu_item(ui, Some("delete"), tr!("remove_from_list"), None, true)
                            .clicked()
                        {
                            *action = Some(Action::Remove(path.clone()));
                            ui.close();
                        }
                    });
                }
            });
    }

    fn projects_action(&mut self, action: Option<Action>) {
        match action {
            Some(Action::Open(path)) => {
                self.show_projects = false;
                self.open_project(path);
            }
            Some(Action::Here(path)) if self.start_screen => self.open_project(path),
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

#[cfg(test)]
#[path = "../../test/app/projects_page/tests.rs"]
mod tests;
