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
/// The least height of the list and the details; they take what the dialog gives them.
const BODY_HEIGHT: f32 = 300.0;
const ROW_HEIGHT: f32 = 52.0;

/// What the list and the detail pane need to know about a built-in plugin.
struct Builtin {
    id: &'static str,
    /// The plugin it is a part of.
    parent: Option<&'static str>,
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    version: &'static str,
    tools: Vec<(&'static str, String)>,
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

/// One row of the list. Returns (row clicked, checkbox clicked). A sub-plugin is drawn under its
/// parent, shifted right, and dimmed while its parent is off (`active`); its check still shows
/// its own switch.
#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &mut egui::Ui,
    t: &Theme,
    icon: Option<&str>,
    name: &str,
    subtitle: &str,
    selected: bool,
    enabled: bool,
    sub: bool,
    active: bool,
) -> (bool, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    ui::paint_row(ui, rect, selected, response.hovered());
    let shift = if sub { 24.0 } else { 0.0 };
    let icon_rect =
        Rect::from_min_size(rect.left_top() + vec2(10.0 + shift, 10.0), vec2(32.0, 32.0));
    plugin_icon(ui, t, icon, name, icon_rect, enabled && active);
    let check =
        Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(16.0, 16.0));
    let text_area = Rect::from_min_max(
        pos2(rect.left() + 52.0 + shift, rect.top()),
        pos2(check.left() - 8.0, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(text_area);
    let (main, sub) = if enabled && active {
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
        let mut builtins: Vec<Builtin> = self
            .builtins
            .iter()
            .filter(|p| !p.core() && !p.external())
            .map(|p| Builtin {
                id: p.id(),
                parent: p.parent(),
                name: p.name(),
                description: p.description(),
                icon: p.icon(),
                version: p.version(),
                tools: p
                    .tool_windows()
                    .iter()
                    .map(|w| (w.title.get(), crate::i18n::shortcut(w.shortcut)))
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
        // Each sub-plugin right under its parent.
        let parents: Vec<&'static str> =
            builtins.iter().map(|b| b.parent.unwrap_or(b.id)).collect();
        let order: Vec<usize> = {
            let mut order: Vec<usize> = (0..builtins.len()).collect();
            let first = |id: &str| parents.iter().position(|p| *p == id).unwrap_or(usize::MAX);
            order.sort_by_key(|&i| (first(parents[i]), builtins[i].parent.is_some()));
            order
        };
        let mut sorted = Vec::with_capacity(builtins.len());
        let mut slots: Vec<Option<Builtin>> = builtins.into_iter().map(Some).collect();
        for i in order {
            sorted.extend(slots[i].take());
        }
        builtins = sorted;

        // Tabs, as in IntelliJ: the installed plugins, and those of the releases of Atelier.
        ui.horizontal(|ui| {
            if tab(ui, &t, tr!("installed"), self.plugin_tab == 0) {
                self.plugin_tab = 0;
            }
            if tab(ui, &t, tr!("plugins_available"), self.plugin_tab == 1) {
                self.plugin_tab = 1;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.plugin_tab == 1 {
                    if ui::icon_button(ui, "refresh", tr!("plugins_catalog_refresh")).clicked() {
                        self.plugin_catalog.refresh();
                    }
                } else if ui::icon_button(ui, "refresh", tr!("reload_project_extensions")).clicked()
                {
                    (self.plugins, self.plugin_errors) = plugins::load(&self.root);
                }
                if ui::icon_button(ui, "add", tr!("install_plugin_file")).clicked() {
                    self.install_plugin_file();
                }
            });
        });
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(line, 0.0, t.separator);
        ui.add_space(8.0);
        self.finish_downloads();
        if self.plugin_tab == 1 {
            self.available_plugins(ui, t);
            return;
        }

        let body_height = (ui.available_height() - 4.0).max(BODY_HEIGHT);
        let (body, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), body_height), Sense::hover());
        let left = Rect::from_min_size(body.min, vec2(LIST_WIDTH, body_height));
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
                tr!("search_plugins"),
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
                .max_height(body_height - 40.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if !visible_builtins.is_empty()
                        && group_header(ui, &t, tr!("builtin_plugins"), visible_builtins.len())
                    {
                        for b in &visible_builtins {
                            let key = format!("b:{}", b.id);
                            let subtitle = trf!("builtin_short_version", version = b.version);
                            let disabled = &self.settings_draft.disabled_builtins;
                            let (clicked, toggle) = list_row(
                                ui,
                                &t,
                                Some(b.icon),
                                b.name,
                                &subtitle,
                                selected == key,
                                !disabled.contains(b.id),
                                b.parent.is_some(),
                                b.parent.is_none_or(|p| !disabled.contains(p)),
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
                        && group_header(ui, &t, tr!("project_extensions"), visible_project.len())
                    {
                        if self.plugins.is_empty() {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.label(
                                    RichText::new(tr!("no_extensions_atelier_plugins"))
                                        .small()
                                        .color(t.text_secondary),
                                );
                            });
                        }
                        for &i in &visible_project {
                            let plugin = &self.plugins[i];
                            let key = format!("p:{}", plugin.id);
                            let enabled = self
                                .settings_draft
                                .enabled_plugins
                                .contains(&self.plugin_key(&plugin.id));
                            let subtitle = if plugin.installed {
                                trf!("installed_plugin_version", version = plugin.version)
                            } else {
                                trf!("project_2", version = plugin.version)
                            };
                            let (clicked, toggle) = list_row(
                                ui,
                                &t,
                                None,
                                &plugin.name,
                                &subtitle,
                                selected == key,
                                enabled,
                                false,
                                true,
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
                            ui.label(RichText::new(tr!("no_results")).color(t.text_secondary));
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
                .max_height(body_height)
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
            match key.strip_prefix("u:") {
                Some(id) => self.uninstall_plugin(id),
                None => self.toggle_plugin(&key),
            }
        }
        for error in &self.plugin_errors {
            ui.colored_label(t.error, error);
        }
    }

    /// The plugins of the releases of Atelier that this IDE loads, to install or update.
    fn available_plugins(&mut self, ui: &mut egui::Ui, t: Theme) {
        if self.plugin_catalog.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        let entries = match self.plugin_catalog.entries() {
            None => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                        ui.label(
                            RichText::new(tr!("plugins_catalog_loading")).color(t.text_secondary),
                        );
                    });
                });
                return;
            }
            Some(Err(e)) => {
                let text = trf!("plugins_catalog_error", e = e);
                ui.add_space(12.0);
                ui.add(egui::Label::new(RichText::new(text).color(t.error)).wrap());
                return;
            }
            Some(Ok(entries)) => entries.clone(),
        };
        if entries.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("plugins_catalog_empty")).color(t.text_secondary));
            });
            return;
        }
        let mut download = None;
        egui::ScrollArea::vertical()
            .id_salt("plugin_catalog")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &entries {
                    let installed = self
                        .plugins
                        .iter()
                        .find(|p| p.id == entry.id && p.installed)
                        .map(|p| p.version.clone());
                    let downloading = self.plugin_catalog.is_downloading(&entry.id);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(4, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&entry.name).font(ui::semibold(14.0)));
                                    ui.label(
                                        RichText::new(trf!(
                                            "plugin_catalog_version",
                                            version = entry.version
                                        ))
                                        .small()
                                        .color(t.text_secondary),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let newer = installed.as_ref().is_none_or(|v| {
                                        plugins::catalog::version_key(v)
                                            < plugins::catalog::version_key(&entry.version)
                                    });
                                    if downloading {
                                        ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                                        ui.label(
                                            RichText::new(tr!("plugin_downloading"))
                                                .color(t.text_secondary),
                                        );
                                    } else if !newer {
                                        ui.label(
                                            RichText::new(tr!("plugin_up_to_date"))
                                                .color(t.text_secondary),
                                        );
                                    } else {
                                        let text = match installed {
                                            Some(_) => {
                                                trf!("plugin_update", version = entry.version)
                                            }
                                            None => tr!("plugin_install").to_string(),
                                        };
                                        if ui::primary_button(ui, &text).clicked() {
                                            download = Some(entry.clone());
                                        }
                                    }
                                    if ui.link(tr!("plugin_release_page")).clicked() {
                                        ui.ctx().open_url(egui::OpenUrl::new_tab(&entry.page));
                                    }
                                });
                            });
                            if !entry.description.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&entry.description).color(t.text_secondary),
                                    )
                                    .wrap(),
                                );
                            }
                        });
                    ui.separator();
                }
            });
        if let Some(entry) = download {
            self.plugin_catalog.download(&entry);
        }
    }

    /// Installs the plugins whose download ended.
    fn finish_downloads(&mut self) {
        for (_, result) in self.plugin_catalog.downloaded() {
            match result {
                Ok(file) => {
                    self.install_plugin_from(&file);
                    let _ = std::fs::remove_file(&file);
                }
                Err(e) => {
                    let title = tr!("plugins").to_string();
                    crate::plugins::builtin::Host::balloon(self, &title, e, true);
                }
            }
        }
    }

    /// Installs a plugin from its file (`.atelier-plugin`), chosen in a dialog, and turns it on.
    fn install_plugin_file(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .add_filter(tr!("plugin_file_filter"), &[plugins::PACKAGE_EXTENSION])
            .pick_file()
        else {
            return;
        };
        self.install_plugin_from(&file);
    }

    /// Installs a plugin from its file, and turns it on.
    fn install_plugin_from(&mut self, file: &std::path::Path) {
        let title = tr!("plugins").to_string();
        match plugins::install(file) {
            Ok(installed) => {
                let text = match installed {
                    plugins::Installed::Now(name) => trf!("plugin_installed", name = name),
                    plugins::Installed::AtNextStart(name) => {
                        trf!("plugin_installed_restart", name = name)
                    }
                };
                self.reload_plugins();
                crate::plugins::builtin::Host::balloon(self, &title, text, false);
            }
            Err(e) => crate::plugins::builtin::Host::balloon(self, &title, e, true),
        }
    }

    /// Uninstalls a plugin of the IDE (not one of the project).
    fn uninstall_plugin(&mut self, id: &str) {
        let Some(name) = (self.plugins.iter())
            .find(|p| p.id == id && p.installed)
            .map(|p| p.name.clone())
        else {
            return;
        };
        let title = tr!("plugins").to_string();
        match plugins::uninstall(id) {
            Ok(gone) => {
                let text = match gone {
                    true => trf!("plugin_uninstalled", name = name),
                    false => trf!("plugin_uninstalled_restart", name = name),
                };
                self.reload_plugins();
                crate::plugins::builtin::Host::balloon(self, &title, text, false);
            }
            Err(e) => crate::plugins::builtin::Host::balloon(self, &title, e, true),
        }
    }

    /// Reads the manifests again, and the state of the plugins in the project: a plugin just
    /// installed is turned on, one uninstalled goes (`sync_native_plugins`).
    fn reload_plugins(&mut self) {
        (self.plugins, self.plugin_errors) = plugins::load(&self.root);
        self.load_project_plugins(!self.start_screen);
    }

    fn toggle_plugin(&mut self, key: &str) {
        match key.split_once(':') {
            // The draft of the settings: Apply or OK makes it so.
            Some(("b", id)) => {
                let disabled = &mut self.settings_draft.disabled_builtins;
                if !disabled.remove(id) {
                    disabled.insert(id.to_string());
                }
            }
            Some(("p", id)) => {
                let key = self.plugin_key(id);
                let enabled = &mut self.settings_draft.enabled_plugins;
                if !enabled.remove(&key) {
                    enabled.insert(key);
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
                ui::button(ui, tr!("disable")).clicked()
            } else {
                ui::primary_button(ui, tr!("enable")).clicked()
            };
            ui.add_space(6.0);
            clicked
        };
        let tabs = |ui: &mut egui::Ui| -> usize {
            let mut current = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(tab_id, || 0usize));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, label) in [tr!("overview"), tr!("contributions")].iter().enumerate() {
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
                let enabled = !self.settings_draft.disabled_builtins.contains(id);
                let meta = trf!("builtin_version", version = b.version);
                if header(ui, Some(b.icon), b.name, meta, enabled) {
                    *toggled = Some(selected.to_string());
                }
                if tabs(ui) == 0 {
                    ui.add(egui::Label::new(b.description).wrap());
                    if let Some(parent) = b.parent.and_then(|p| builtins.iter().find(|o| o.id == p))
                    {
                        ui.add_space(10.0);
                        let part = trf!("plugin_part_of", parent = parent.name);
                        ui.label(RichText::new(part).color(t.text_secondary));
                        if self.settings_draft.disabled_builtins.contains(parent.id) {
                            let off = trf!("plugin_parent_off", parent = parent.name);
                            ui.label(RichText::new(off).color(t.text_secondary));
                        }
                    }
                    if self.auto_plugins.contains(id) {
                        // Git while the project leaves it to the IDE (`"auto"`).
                        ui.add_space(10.0);
                        let auto = RichText::new(tr!("plugin_auto_git")).color(t.text_secondary);
                        ui.add(egui::Label::new(auto).wrap());
                    }
                    if !enabled {
                        ui.add_space(10.0);
                        ui.label(RichText::new(tr!("plugin_disabled")).color(t.text_secondary));
                    }
                } else {
                    if b.tools.is_empty() && b.commands.is_empty() {
                        ui.label(
                            RichText::new(tr!("plugin_contributes_no_window_no"))
                                .color(t.text_secondary),
                        );
                    }
                    if !b.tools.is_empty() {
                        ui::section(ui, tr!("tool_windows"));
                        for (title, shortcut) in &b.tools {
                            contribution(ui, &t, title, shortcut);
                        }
                        ui.add_space(8.0);
                    }
                    if !b.commands.is_empty() {
                        ui::section(ui, tr!("search_everywhere_actions"));
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
                let enabled = self
                    .settings_draft
                    .enabled_plugins
                    .contains(&self.plugin_key(id));
                let meta = trf!("project_extension_version", version = plugin.version);
                if header(ui, None, &plugin.name, meta, enabled) {
                    *toggled = Some(selected.to_string());
                }
                if tabs(ui) == 0 {
                    ui.add(egui::Label::new(&plugin.description).wrap());
                    ui.add_space(10.0);
                    if let Some(abi) = plugin.abi {
                        ui.label(
                            RichText::new(trf!("installed_plugin_interface", abi = abi))
                                .small()
                                .color(t.text_secondary),
                        );
                        ui.add_space(4.0);
                    }
                    if plugin.installed && ui::button(ui, tr!("plugin_uninstall")).clicked() {
                        // Handled with the toggles, once the page is drawn.
                        *toggled = Some(format!("u:{id}"));
                    }
                    if plugin.installed {
                        ui.add_space(8.0);
                    }
                    let warning = match plugin.library_path() {
                        Some(library) => {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(trf!(
                                        "native_plugin_library",
                                        path = library.display()
                                    ))
                                    .small()
                                    .monospace(),
                                )
                                .wrap(),
                            );
                            ui.add_space(4.0);
                            tr!("native_plugin_warning")
                        }
                        None => tr!("these_commands_run_your_local"),
                    };
                    ui.add(
                        egui::Label::new(RichText::new(warning).small().color(t.text_secondary))
                            .wrap(),
                    );
                } else {
                    ui::section(ui, tr!("commands"));
                    if !enabled {
                        ui.label(
                            RichText::new(tr!("enable_extension_run_commands"))
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
