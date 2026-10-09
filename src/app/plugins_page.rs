//! The Plugins page of the settings, laid out as IntelliJ's: tabs on top, a searchable list of
//! installed plugins in collapsible groups on the left, the details of the selected one on the
//! right.

use super::Ide;
use crate::{
    plugins,
    ui::{self, Theme},
};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind,
    UiBuilder, pos2, vec2,
};

const LIST_WIDTH: f32 = 236.0;
const BODY_HEIGHT: f32 = 450.0;
const ROW_HEIGHT: f32 = 52.0;

/// What the list and the detail pane need to know about a built-in plugin.
struct Builtin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    version: &'static str,
    tools: Vec<(&'static str, &'static str)>,
    /// (label, shortcut)
    commands: Vec<(String, String)>,
}

fn matches(filter: &str, name: &str, description: &str) -> bool {
    filter.is_empty()
        || name.to_lowercase().contains(filter)
        || description.to_lowercase().contains(filter)
}

/// A child `Ui` that fills `rect` top to bottom, whatever the layout of its parent.
fn child(ui: &mut egui::Ui, rect: Rect) -> egui::Ui {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}

fn paint_check(ui: &egui::Ui, t: &Theme, rect: Rect, checked: bool) {
    let painter = ui.painter();
    if checked {
        painter.rect_filled(rect, 3.0, t.accent);
        let (x, y) = (rect.left(), rect.top());
        painter.add(egui::Shape::line(
            vec![
                pos2(x + 3.5, y + 8.5),
                pos2(x + 6.5, y + 11.5),
                pos2(x + 12.5, y + 4.5),
            ],
            Stroke::new(1.8_f32, Color32::WHITE),
        ));
    } else {
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0_f32, t.border),
            StrokeKind::Inside,
        );
    }
}

/// The icon of a plugin: its own, or a colored square with its initial.
fn plugin_icon(
    ui: &mut egui::Ui,
    t: &Theme,
    icon: Option<&str>,
    name: &str,
    rect: Rect,
    enabled: bool,
) {
    match icon {
        Some(icon) => {
            let image = ui::icon(ui, icon).fit_to_exact_size(rect.size());
            let image = if enabled {
                image
            } else {
                image.tint(t.text_disabled)
            };
            image.paint_at(ui, rect.shrink(rect.width() * 0.1));
        }
        None => {
            let fill = if enabled {
                t.accent
            } else {
                t.accent.gamma_multiply(0.4)
            };
            let painter = ui.painter();
            painter.rect_filled(rect, rect.width() * 0.2, fill);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                name.chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
                ui::semibold(rect.width() * 0.5),
                Color32::WHITE,
            );
        }
    }
}

/// One row of the list. Returns (row clicked, checkbox clicked).
#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &mut egui::Ui,
    t: &Theme,
    icon: Option<&str>,
    name: &str,
    subtitle: &str,
    selected: bool,
    enabled: bool,
) -> (bool, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    ui::paint_row(ui, rect, selected, response.hovered());
    let icon_rect = Rect::from_min_size(rect.left_top() + vec2(10.0, 10.0), vec2(32.0, 32.0));
    plugin_icon(ui, t, icon, name, icon_rect, enabled);
    let check =
        Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(16.0, 16.0));
    let text_area = Rect::from_min_max(
        pos2(rect.left() + 52.0, rect.top()),
        pos2(check.left() - 8.0, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(text_area);
    let (main, sub) = if enabled {
        (t.text, t.text_secondary)
    } else {
        (t.text_disabled, t.text_disabled)
    };
    painter.text(
        pos2(text_area.left(), rect.top() + 11.0),
        Align2::LEFT_TOP,
        name,
        ui::semibold(13.0),
        main,
    );
    painter.text(
        pos2(text_area.left(), rect.top() + 29.0),
        Align2::LEFT_TOP,
        subtitle,
        FontId::proportional(11.5),
        sub,
    );
    let toggle = ui.interact(
        check.expand(6.0),
        response.id.with("toggle"),
        Sense::click(),
    );
    paint_check(ui, t, check, enabled);
    (response.clicked() && !toggle.clicked(), toggle.clicked())
}

/// Collapsible group title. Returns whether the group is open.
fn group_header(ui: &mut egui::Ui, t: &Theme, title: &str, count: usize) -> bool {
    let id = ui.make_persistent_id(("plugin_group", title));
    let mut open = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || true));
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    ui::paint_row(ui, rect, false, response.hovered());
    ui::paint_chevron(ui, pos2(rect.left() + 16.0, rect.center().y), open);
    let x = ui::paint_text(
        ui,
        rect,
        rect.left() + 30.0,
        title,
        t.text,
        ui::semibold(12.5),
    );
    ui::paint_text(
        ui,
        rect,
        x + 6.0,
        &count.to_string(),
        t.text_secondary,
        FontId::proportional(12.0),
    );
    open
}

/// Text tab with an accent underline when selected. Returns true when clicked.
fn tab(ui: &mut egui::Ui, t: &Theme, text: &str, selected: bool) -> bool {
    let galley = ui.painter().layout_no_wrap(
        text.into(),
        if selected {
            ui::semibold(13.0)
        } else {
            FontId::proportional(13.0)
        },
        if selected { t.text } else { t.text_secondary },
    );
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 24.0, 30.0), Sense::click());
    if response.hovered() && !selected {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(0.0, 2.0)), 5.0, t.hover);
    }
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0 - 1.0,
        ),
        galley,
        t.text,
    );
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_max(
                pos2(rect.left() + 8.0, rect.bottom() - 3.0),
                pos2(rect.right() - 8.0, rect.bottom() - 1.0),
            ),
            1.0,
            t.accent,
        );
    }
    response.clicked()
}

/// A shortcut drawn as a small rounded key.
fn key_badge(ui: &mut egui::Ui, t: &Theme, text: &str) {
    let galley =
        ui.painter()
            .layout_no_wrap(text.into(), FontId::proportional(11.0), t.text_secondary);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(12.0, 6.0), Sense::hover());
    ui.painter().rect(
        rect,
        4.0,
        t.hover,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.min + vec2(6.0, 3.0), galley, t.text_secondary);
}

/// A line of the contributions list: label on the left, shortcut on the right.
fn contribution(ui: &mut egui::Ui, t: &Theme, label: &str, shortcut: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        ui.label(label);
        if !shortcut.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                key_badge(ui, t, shortcut)
            });
        }
    });
}

impl Ide {
    /// Draws the page. `command` receives the shell command of a project plugin to run.
    pub(super) fn plugins_page(
        &mut self,
        ui: &mut egui::Ui,
        t: Theme,
        command: &mut Option<String>,
    ) {
        let builtins: Vec<Builtin> = self
            .builtins
            .iter()
            .filter(|p| !p.core())
            .map(|p| Builtin {
                id: p.id(),
                name: p.name(),
                description: p.description(),
                icon: p.icon(),
                version: p.version(),
                tools: p
                    .tool_windows()
                    .iter()
                    .map(|w| (w.title, w.shortcut))
                    .collect(),
                commands: p
                    .commands()
                    .into_iter()
                    .map(|c| {
                        let (label, shortcut) = c.label.split_once("  ").unwrap_or((&c.label, ""));
                        (label.to_string(), shortcut.to_string())
                    })
                    .collect(),
            })
            .collect();

        // Tabs, as in IntelliJ. Only the installed plugins exist here: there is no marketplace.
        ui.horizontal(|ui| {
            tab(ui, &t, "Installés", true);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::icon_button(ui, "refresh", "Recharger les extensions du projet").clicked() {
                    (self.plugins, self.plugin_errors) = plugins::load(&self.root);
                }
            });
        });
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(line, 0.0, t.separator);
        ui.add_space(8.0);

        let (body, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), BODY_HEIGHT), Sense::hover());
        let left = Rect::from_min_size(body.min, vec2(LIST_WIDTH, BODY_HEIGHT));
        let right = Rect::from_min_max(pos2(left.right() + 20.0, body.top()), body.max);
        ui.painter().rect_filled(
            Rect::from_min_max(
                pos2(left.right() + 9.0, body.top()),
                pos2(left.right() + 10.0, body.bottom()),
            ),
            0.0,
            t.separator,
        );

        let filter = self.plugin_filter.to_lowercase();
        // Keep the selection on something that exists.
        let exists = |key: &str, ide: &Ide| match key.split_once(':') {
            Some(("b", id)) => ide.builtins.iter().any(|p| p.id() == id),
            Some(("p", id)) => ide.plugins.iter().any(|p| p.id == id),
            _ => false,
        };
        if !self
            .plugin_selected
            .as_deref()
            .is_some_and(|k| exists(k, self))
        {
            self.plugin_selected = builtins.first().map(|b| format!("b:{}", b.id));
        }
        let selected = self.plugin_selected.clone().unwrap_or_default();
        let mut select = None;
        let mut toggled: Option<String> = None;

        // ---- List ----
        let mut list = child(ui, left);
        {
            let ui = &mut list;
            ui::search_field(
                ui,
                &mut self.plugin_filter,
                "Rechercher parmi les plugins",
                LIST_WIDTH,
            );
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let visible_builtins: Vec<&Builtin> = builtins
                .iter()
                .filter(|b| matches(&filter, b.name, b.description))
                .collect();
            let visible_project: Vec<usize> = self
                .plugins
                .iter()
                .enumerate()
                .filter(|(_, p)| matches(&filter, &p.name, &p.description))
                .map(|(i, _)| i)
                .collect();
            egui::ScrollArea::vertical()
                .id_salt("plugin_list")
                .max_height(BODY_HEIGHT - 40.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if !visible_builtins.is_empty()
                        && group_header(ui, &t, "Intégrés", visible_builtins.len())
                    {
                        for b in &visible_builtins {
                            let key = format!("b:{}", b.id);
                            let subtitle = format!("Intégré · {}", b.version);
                            let (clicked, toggle) = list_row(
                                ui,
                                &t,
                                Some(b.icon),
                                b.name,
                                &subtitle,
                                selected == key,
                                self.builtin_enabled(b.id),
                            );
                            if toggle {
                                toggled = Some(key.clone());
                            }
                            if clicked || toggle {
                                select = Some(key);
                            }
                        }
                    }
                    if (!visible_project.is_empty() || filter.is_empty())
                        && group_header(ui, &t, "Extensions du projet", visible_project.len())
                    {
                        if self.plugins.is_empty() {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.label(
                                    RichText::new("Aucune extension dans .atelier/plugins/")
                                        .small()
                                        .color(t.text_secondary),
                                );
                            });
                        }
                        for &i in &visible_project {
                            let plugin = &self.plugins[i];
                            let key = format!("p:{}", plugin.id);
                            let enabled = self
                                .settings
                                .enabled_plugins
                                .contains(&self.plugin_key(&plugin.id));
                            let subtitle = format!("Projet · {}", plugin.version);
                            let (clicked, toggle) = list_row(
                                ui,
                                &t,
                                None,
                                &plugin.name,
                                &subtitle,
                                selected == key,
                                enabled,
                            );
                            if toggle {
                                toggled = Some(key.clone());
                            }
                            if clicked || toggle {
                                select = Some(key);
                            }
                        }
                    }
                    if visible_builtins.is_empty() && visible_project.is_empty() {
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Aucun résultat").color(t.text_secondary));
                        });
                    }
                });
        }

        // ---- Details ----
        let mut detail = child(ui, right);
        {
            let ui = &mut detail;
            egui::ScrollArea::vertical()
                .id_salt("plugin_detail")
                .max_height(BODY_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(right.width());
                    self.plugin_detail(ui, t, &builtins, &selected, &mut toggled, command);
                });
        }

        if let Some(key) = select {
            self.plugin_selected = Some(key);
        }
        if let Some(key) = toggled {
            self.toggle_plugin(&key);
        }
        for error in &self.plugin_errors {
            ui.colored_label(t.error, error);
        }
    }

    fn toggle_plugin(&mut self, key: &str) {
        match key.split_once(':') {
            Some(("b", id)) => {
                let enabled = self.builtin_enabled(id);
                self.set_builtin_enabled(id, !enabled);
            }
            Some(("p", id)) => {
                let key = self.plugin_key(id);
                if !self.settings.enabled_plugins.remove(&key) {
                    self.settings.enabled_plugins.insert(key);
                }
            }
            _ => {}
        }
    }

    fn plugin_detail(
        &self,
        ui: &mut egui::Ui,
        t: Theme,
        builtins: &[Builtin],
        selected: &str,
        toggled: &mut Option<String>,
        command: &mut Option<String>,
    ) {
        let tab_id = ui.make_persistent_id(("plugin_tab", selected));
        // Title, version and the enable button.
        let header = |ui: &mut egui::Ui,
                      icon: Option<&str>,
                      name: &str,
                      meta: String,
                      enabled: bool|
         -> bool {
            let mut clicked = false;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                plugin_icon(ui, &t, icon, name, rect, enabled);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(name).font(ui::semibold(20.0)));
                    ui.label(RichText::new(meta).small().color(t.text_secondary));
                });
            });
            ui.add_space(10.0);
            clicked |= if enabled {
                ui::button(ui, "Désactiver").clicked()
            } else {
                ui::primary_button(ui, "Activer").clicked()
            };
            ui.add_space(6.0);
            clicked
        };
        let tabs = |ui: &mut egui::Ui| -> usize {
            let mut current = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(tab_id, || 0usize));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, label) in ["Aperçu", "Contributions"].iter().enumerate() {
                    if tab(ui, &t, label, current == i) {
                        current = i;
                    }
                }
            });
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(line, 0.0, t.separator);
            ui.add_space(8.0);
            ui.data_mut(|d| d.insert_temp(tab_id, current));
            current
        };
        match selected.split_once(':') {
            Some(("b", id)) => {
                let Some(b) = builtins.iter().find(|b| b.id == id) else {
                    return;
                };
                let enabled = self.builtin_enabled(id);
                let meta = format!("Intégré  ·  version {}", b.version);
                if header(ui, Some(b.icon), b.name, meta, enabled) {
                    *toggled = Some(selected.to_string());
                }
                if tabs(ui) == 0 {
                    ui.add(egui::Label::new(b.description).wrap());
                    if !enabled {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Ce plugin est désactivé.").color(t.text_secondary));
                    }
                } else {
                    if b.tools.is_empty() && b.commands.is_empty() {
                        ui.label(
                            RichText::new("Ce plugin ne contribue ni fenêtre ni action.")
                                .color(t.text_secondary),
                        );
                    }
                    if !b.tools.is_empty() {
                        ui::section(ui, "Fenêtres d'outils");
                        for (title, shortcut) in &b.tools {
                            contribution(ui, &t, title, shortcut);
                        }
                        ui.add_space(8.0);
                    }
                    if !b.commands.is_empty() {
                        ui::section(ui, "Actions de Rechercher partout");
                        for (label, shortcut) in &b.commands {
                            contribution(ui, &t, label, shortcut);
                        }
                    }
                }
            }
            Some(("p", id)) => {
                let Some(plugin) = self.plugins.iter().find(|p| p.id == id) else {
                    return;
                };
                let enabled = self.settings.enabled_plugins.contains(&self.plugin_key(id));
                let meta = format!("Extension du projet  ·  version {}", plugin.version);
                if header(ui, None, &plugin.name, meta, enabled) {
                    *toggled = Some(selected.to_string());
                }
                if tabs(ui) == 0 {
                    ui.add(egui::Label::new(&plugin.description).wrap());
                    ui.add_space(10.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Ces commandes s'exécutent avec vos droits locaux : n'activez que des extensions de confiance.",
                            )
                            .small()
                            .color(t.text_secondary),
                        )
                        .wrap(),
                    );
                } else {
                    ui::section(ui, "Commandes");
                    if !enabled {
                        ui.label(
                            RichText::new("Activez l'extension pour lancer ses commandes.")
                                .color(t.text_secondary),
                        );
                    }
                    for cmd in &plugin.commands {
                        let shell = plugins::shell_command(&cmd.program, &cmd.args);
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(enabled, |ui| {
                                if ui::button(ui, &cmd.name).on_hover_text(&shell).clicked() {
                                    *command = Some(shell.clone());
                                }
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&shell)
                                        .small()
                                        .monospace()
                                        .color(t.text_secondary),
                                )
                                .truncate(),
                            );
                        });
                    }
                }
            }
            _ => {}
        }
    }
}
