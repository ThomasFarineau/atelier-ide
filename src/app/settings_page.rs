//! The Settings dialog, laid out as IntelliJ's: a searchable tree of pages on the left, the page
//! under its breadcrumb on the right, and OK, Cancel and Apply at the bottom. The pages edit a
//! draft of the settings: nothing changes before Apply or OK, and Cancel forgets the draft.

use super::{Ide, SHORTCUTS, Settings};
use crate::ui::{self, Theme};
use eframe::egui::{self, Align, FontId, Layout, Rect, RichText, Sense, UiBuilder, pos2, vec2};
use std::collections::BTreeSet;

const TREE_WIDTH: f32 = 264.0;
const TITLE_HEIGHT: f32 = 40.0;
const TREE_ROW: f32 = 26.0;
const LABEL_WIDTH: f32 = 130.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(super) enum Page {
    AppearanceBehavior,
    Appearance,
    LanguageRegion,
    Keymap,
    Editor,
    EditorGeneral,
    Fonts,
    Plugins,
    Languages,
    JavaScript,
    Rust,
    Build,
    RunTools,
    Sync,
}

impl Page {
    /// The tree, in the order it is shown: each page and the group it belongs to.
    pub(super) const TREE: [(Page, Option<Page>); 14] = [
        (Page::AppearanceBehavior, None),
        (Page::Appearance, Some(Page::AppearanceBehavior)),
        (Page::Fonts, Some(Page::AppearanceBehavior)),
        (Page::LanguageRegion, Some(Page::AppearanceBehavior)),
        (Page::Keymap, None),
        (Page::Editor, None),
        (Page::EditorGeneral, Some(Page::Editor)),
        (Page::Plugins, None),
        (Page::Languages, None),
        (Page::JavaScript, Some(Page::Languages)),
        (Page::Rust, Some(Page::Languages)),
        (Page::Build, None),
        (Page::RunTools, Some(Page::Build)),
        (Page::Sync, None),
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Page::AppearanceBehavior => tr!("appearance_behavior"),
            Page::Appearance => tr!("appearance"),
            Page::LanguageRegion => tr!("language_region"),
            Page::Keymap => tr!("keymap"),
            Page::Editor => tr!("editor"),
            Page::EditorGeneral => tr!("general"),
            Page::Fonts => tr!("fonts"),
            Page::Plugins => tr!("plugins"),
            Page::Languages => tr!("languages_frameworks"),
            Page::JavaScript => tr!("javascript_typescript"),
            Page::Rust => tr!("rust"),
            Page::Build => tr!("build_execution_deployment"),
            Page::RunTools => tr!("run_tools"),
            Page::Sync => tr!("settings_sync"),
        }
    }

    fn parent(self) -> Option<Page> {
        Page::TREE
            .iter()
            .find(|(p, _)| *p == self)
            .and_then(|(_, parent)| *parent)
    }

    fn children(self) -> impl Iterator<Item = Page> {
        Page::TREE
            .iter()
            .filter(move |(_, parent)| *parent == Some(self))
            .map(|(p, _)| *p)
    }

    fn is_group(self) -> bool {
        self.children().next().is_some()
    }

    /// The words of the options of the page, for the search of the tree.
    fn keywords(self) -> Vec<&'static str> {
        match self {
            Page::Appearance => vec![tr!("theme"), "Islands", "Dark", "Light"],
            Page::LanguageRegion => {
                let mut words = vec![tr!("interface_language")];
                words.extend(
                    crate::i18n::identifiers()
                        .into_iter()
                        .map(crate::i18n::name),
                );
                words
            }
            Page::Keymap => SHORTCUTS.iter().map(|(_, action)| action.get()).collect(),
            Page::EditorGeneral => vec![
                tr!("mouse_control"),
                tr!("wheel"),
                tr!("zoom"),
                tr!("code_display"),
                tr!("inlay_hints"),
                tr!("sticky_lines"),
                tr!("breadcrumbs"),
                tr!("spell_checking"),
                tr!("check_before_commit"),
            ],
            Page::Fonts => vec![
                tr!("font_2"),
                tr!("size"),
                tr!("zoom"),
                "Source Sans 3",
                "Inter",
                "Source Code Pro",
                "JetBrains Mono",
                "Google Fonts",
                "Fontsource",
            ],
            Page::Plugins => vec![tr!("extensions"), "Git", "Markdown"],
            Page::JavaScript => vec![
                "TypeScript",
                "typescript-language-server",
                tr!("language_server"),
            ],
            Page::Rust => vec!["rust-analyzer", "cargo", tr!("language_server")],
            Page::RunTools => vec!["cargo", "python", "java", "node", "tsx", "F5"],
            Page::Sync => vec![
                tr!("export_settings"),
                tr!("import_settings"),
                tr!("sync_folder"),
                "OneDrive",
                "Dropbox",
            ],
            _ => Vec::new(),
        }
    }

    fn matches(self, filter: &str) -> bool {
        filter.is_empty()
            || self.title().to_lowercase().contains(filter)
            || self
                .keywords()
                .iter()
                .any(|k| k.to_lowercase().contains(filter))
    }
}

/// What the pages edit before it is applied.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Draft {
    pub theme: usize,
    pub font_size: f32,
    pub wheel_zoom_off: bool,
    pub inlay_hints_off: bool,
    pub sticky_lines_off: bool,
    pub breadcrumbs_off: bool,
    pub spellcheck_off: bool,
    pub commit_check_off: bool,
    pub fonts: ui::Fonts,
    /// Identifier of the language of the interface.
    pub language: String,
    pub disabled_builtins: BTreeSet<String>,
    pub enabled_plugins: BTreeSet<String>,
}

impl Draft {
    pub(super) fn of(settings: &Settings) -> Self {
        Self {
            theme: settings.theme,
            font_size: settings.font_size,
            wheel_zoom_off: settings.wheel_zoom_off,
            inlay_hints_off: settings.inlay_hints_off,
            sticky_lines_off: settings.sticky_lines_off,
            breadcrumbs_off: settings.breadcrumbs_off,
            spellcheck_off: settings.spellcheck_off,
            commit_check_off: settings.commit_check_off,
            fonts: settings.fonts.clone(),
            language: crate::i18n::current().to_string(),
            disabled_builtins: settings.disabled_builtins.clone(),
            enabled_plugins: settings.enabled_plugins.clone(),
        }
    }

    /// Whether the options of a page differ from `other`.
    fn page_differs(&self, other: &Draft, page: Page) -> bool {
        match page {
            Page::Appearance => self.theme != other.theme,
            Page::LanguageRegion => self.language != other.language,
            Page::EditorGeneral => {
                self.wheel_zoom_off != other.wheel_zoom_off
                    || self.inlay_hints_off != other.inlay_hints_off
                    || self.sticky_lines_off != other.sticky_lines_off
                    || self.breadcrumbs_off != other.breadcrumbs_off
                    || self.spellcheck_off != other.spellcheck_off
                    || self.commit_check_off != other.commit_check_off
            }
            Page::Fonts => self.font_size != other.font_size || self.fonts != other.fonts,
            Page::Plugins => {
                self.disabled_builtins != other.disabled_builtins
                    || self.enabled_plugins != other.enabled_plugins
            }
            _ => false,
        }
    }

    /// Puts back the options of a page as they are in `other`.
    fn reset_page(&mut self, other: &Draft, page: Page) {
        match page {
            Page::Appearance => self.theme = other.theme,
            Page::LanguageRegion => self.language = other.language.clone(),
            Page::EditorGeneral => {
                self.wheel_zoom_off = other.wheel_zoom_off;
                self.inlay_hints_off = other.inlay_hints_off;
                self.sticky_lines_off = other.sticky_lines_off;
                self.breadcrumbs_off = other.breadcrumbs_off;
                self.spellcheck_off = other.spellcheck_off;
                self.commit_check_off = other.commit_check_off;
            }
            Page::Fonts => {
                self.font_size = other.font_size;
                self.fonts = other.fonts.clone();
            }
            Page::Plugins => {
                self.disabled_builtins = other.disabled_builtins.clone();
                self.enabled_plugins = other.enabled_plugins.clone();
            }
            _ => {}
        }
    }
}

/// A tool F5 runs, and whether it is on the PATH.
#[derive(Clone, Debug)]
pub(super) struct RunTool {
    pub language: &'static str,
    pub command: &'static str,
    pub found: bool,
}

/// Whether a program can be started by its name.
fn on_path(program: &str) -> bool {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat"]
            .iter()
            .map(|ext| format!("{program}.{ext}"))
            .collect()
    } else {
        vec![program.to_string()]
    };
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
    })
}

pub(super) fn run_tools() -> Vec<RunTool> {
    [
        ("Rust", "cargo run", "cargo"),
        ("Python", "python -u <file>", "python"),
        ("Java", "java <file>", "java"),
        ("JavaScript", "node <file>", "node"),
        ("TypeScript", "tsx <file>", "tsx"),
    ]
    .into_iter()
    .map(|(language, command, program)| RunTool {
        language,
        command,
        found: on_path(program),
    })
    .collect()
}

/// A child `Ui` that fills `rect` top to bottom.
fn child(ui: &mut egui::Ui, rect: Rect) -> egui::Ui {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}

/// A line of a form: the label in a column, then the control.
pub(super) fn form_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, 24.0), Sense::hover());
        let t = ui::theme(ui.ctx());
        ui::paint_text(
            ui,
            rect,
            rect.left(),
            label,
            t.text,
            FontId::proportional(13.0),
        );
        add(ui)
    })
    .inner
}

/// A titled group of options, as IntelliJ separates them: a title and a line.
pub(super) fn group(ui: &mut egui::Ui, t: &Theme, title: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(ui::semibold(13.0)));
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
        ui.painter().hline(
            rect.left() + 6.0..=rect.right(),
            rect.center().y,
            egui::Stroke::new(1.0_f32, t.separator),
        );
    });
    ui.add_space(2.0);
}

pub(super) fn note(ui: &mut egui::Ui, t: &Theme, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(t.text_secondary)).wrap());
}

impl Ide {
    /// Opens the dialog on a page, with a fresh draft of the settings.
    pub(super) fn open_settings(&mut self, page: Page) {
        self.settings_page = page;
        self.settings_draft = Draft::of(&self.settings);
        self.settings_original = self.settings_draft.clone();
        self.settings_filter.clear();
        self.run_tools = run_tools();
        self.show_settings = true;
        self.settings_fresh = true;
    }

    /// Makes the draft the settings.
    pub(super) fn apply_settings(&mut self, ctx: &egui::Context) {
        let draft = self.settings_draft.clone();
        let original = self.settings_original.clone();
        if draft.theme != original.theme
            || draft.font_size != original.font_size
            || draft.fonts != original.fonts
        {
            self.settings.theme = draft.theme;
            self.settings.font_size = draft.font_size;
            self.settings.fonts = draft.fonts.clone();
            self.apply_theme(ctx);
        }
        self.settings.wheel_zoom_off = draft.wheel_zoom_off;
        self.settings.inlay_hints_off = draft.inlay_hints_off;
        self.settings.sticky_lines_off = draft.sticky_lines_off;
        self.settings.breadcrumbs_off = draft.breadcrumbs_off;
        self.settings.spellcheck_off = draft.spellcheck_off;
        self.settings.commit_check_off = draft.commit_check_off;
        if draft.language != original.language {
            self.settings.language = Some(draft.language.clone());
            crate::i18n::set(&draft.language);
        }
        let ids: Vec<&'static str> = self.builtins.iter().map(|p| p.id()).collect();
        for id in ids {
            let enabled = !draft.disabled_builtins.contains(id);
            if enabled != self.builtin_enabled(id) {
                self.set_builtin_enabled(id, enabled);
            }
        }
        self.settings.enabled_plugins = draft.enabled_plugins.clone();
        self.settings_original = draft;
        // At once: a window opened now reads the file.
        if let Err(e) = self.persist_settings() {
            self.status = trf!("error_detail", e = format!("{:#}", e));
        }
        self.sync_out();
        ctx.request_repaint();
    }

    /// Export and import of the settings, and the sync folder.
    fn sync_page(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let ctx = ui.ctx().clone();
        note(ui, t, tr!("settings_sync_hint"));
        ui.add_space(8.0);
        group(ui, t, tr!("export_import"));
        ui.horizontal(|ui| {
            if ui::button(ui, tr!("export_settings")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name(super::portable::FILE)
                    .add_filter("JSON", &["json"])
                    .save_file()
            {
                self.status = match self.export_settings_to(&path) {
                    Ok(()) => trf!("settings_exported", path = path.display()),
                    Err(e) => trf!("error_detail", e = format!("{e:#}")),
                };
            }
            if ui::button(ui, tr!("import_settings")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
            {
                self.status = match self.import_settings_from(&path, &ctx) {
                    Ok(n) => trf!("settings_imported", count = n),
                    Err(e) => trf!("error_detail", e = format!("{e:#}")),
                };
            }
        });
        ui.add_space(12.0);
        group(ui, t, tr!("sync_folder"));
        let shown = self
            .settings
            .sync_folder
            .as_ref()
            .map_or_else(|| tr!("no_sync_folder").to_string(), |p| p.display().to_string());
        ui.label(RichText::new(shown).color(t.text_secondary));
        ui.horizontal(|ui| {
            if ui::button(ui, tr!("choose_folder")).clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                self.settings.sync_folder = Some(folder);
                // What is in the folder wins when it is newer, else this machine writes it.
                self.sync_in(&ctx);
                self.sync_out();
                let _ = self.persist_settings();
            }
            if self.settings.sync_folder.is_some() {
                if ui::button(ui, tr!("sync_now")).clicked() {
                    self.sync_in(&ctx);
                    self.sync_out();
                }
                if ui::button(ui, tr!("stop_sync")).clicked() {
                    self.settings.sync_folder = None;
                    let _ = self.persist_settings();
                }
            }
        });
    }

    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let t = self.theme(ctx);
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() - 80.0).clamp(760.0, 1040.0),
            (screen.height() - 80.0).clamp(520.0, 720.0),
        );
        let mut command = None;
        let mut builtin_command = None;
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let modal = egui::Modal::new(egui::Id::new("settings"))
            .frame(
                egui::Frame::new()
                    .fill(t.popup)
                    .corner_radius(10)
                    .stroke(egui::Stroke::new(1.0_f32, t.border)),
            )
            .show(ctx, |ui| {
                ui.set_width(size.x);
                ui.set_height(size.y);
                let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
                let title =
                    Rect::from_min_max(outer.min, pos2(outer.right(), outer.top() + TITLE_HEIGHT));
                let footer =
                    Rect::from_min_max(pos2(outer.left(), outer.bottom() - 56.0), outer.max);
                let tree = Rect::from_min_max(
                    pos2(outer.left(), title.bottom()),
                    pos2(outer.left() + TREE_WIDTH, footer.top()),
                );
                let page = Rect::from_min_max(
                    pos2(tree.right() + 1.0, title.bottom()),
                    pos2(outer.right(), footer.top()),
                );
                // The title bar of the dialog, as the New UI draws it, with its close button.
                ui.painter().hline(
                    outer.left()..=outer.right(),
                    title.bottom(),
                    egui::Stroke::new(1.0_f32, t.separator),
                );
                ui::paint_text(
                    ui,
                    title,
                    title.left() + 16.0,
                    tr!("settings"),
                    t.text,
                    ui::semibold(13.5),
                );
                let mut close_ui = child(
                    ui,
                    Rect::from_min_max(
                        pos2(title.right() - 44.0, title.top() + 6.0),
                        pos2(title.right() - 8.0, title.bottom() - 6.0),
                    ),
                );
                cancel |= ui::icon_button(&mut close_ui, "close", tr!("close")).clicked();
                // The tree sits on the darker background, as in IntelliJ.
                ui.painter().rect_filled(tree, 0.0, t.frame);
                ui.painter().vline(
                    tree.right(),
                    tree.top()..=tree.bottom(),
                    egui::Stroke::new(1.0_f32, t.separator),
                );
                ui.painter().hline(
                    outer.left()..=outer.right(),
                    footer.top(),
                    egui::Stroke::new(1.0_f32, t.separator),
                );
                let mut tree_ui = child(ui, tree.shrink2(vec2(8.0, 10.0)));
                self.settings_tree(&mut tree_ui, &t);
                let mut page_ui = child(ui, page.shrink2(vec2(20.0, 12.0)));
                self.settings_page_view(&mut page_ui, &t, &mut command, &mut builtin_command);
                let mut footer_ui = child(ui, footer.shrink2(vec2(16.0, 14.0)));
                footer_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let modified = self.settings_draft != self.settings_original;
                    apply = ui
                        .add_enabled_ui(modified, |ui| ui::button(ui, tr!("apply")))
                        .inner
                        .clicked();
                    cancel |= ui::button(ui, tr!("cancel")).clicked();
                    ok = ui::primary_button(ui, tr!("ok")).clicked();
                });
            });
        // Escape cancels; Enter confirms unless a field takes it.
        let typing = ctx.wants_keyboard_input();
        cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape)) || modal.should_close();
        let fresh = std::mem::take(&mut self.settings_fresh);
        ok |= !typing && !fresh && ctx.input(|i| i.key_pressed(egui::Key::Enter));
        if apply || ok {
            self.apply_settings(ctx);
        }
        if ok || cancel {
            self.show_settings = false;
        }
        if let Some((plugin, id)) = builtin_command {
            self.run_builtin_command(plugin, id);
        }
        if let Some(cmd) = command {
            self.show_settings = false;
            self.execute(&cmd);
        }
    }

    fn settings_tree(&mut self, ui: &mut egui::Ui, t: &Theme) {
        ui::search_field(ui, &mut self.settings_filter, "", ui.available_width());
        ui.add_space(8.0);
        let filter = self.settings_filter.trim().to_lowercase();
        // A page is shown when it matches, when its group matches, or when one of its pages does.
        let shown = |page: Page| {
            page.matches(&filter)
                || page.parent().is_some_and(|p| p.matches(&filter))
                || page.children().any(|c| c.matches(&filter))
        };
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut select = None;
        egui::ScrollArea::vertical()
            .id_salt("settings_tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (page, parent) in Page::TREE {
                    if !shown(page) {
                        continue;
                    }
                    let open_id = ui.make_persistent_id(("settings_group", parent.unwrap_or(page)));
                    // A page of a closed group is hidden, unless the search shows it.
                    if parent.is_some() {
                        let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);
                        if !open && filter.is_empty() && self.settings_page != page {
                            continue;
                        }
                    }
                    let depth = if parent.is_some() { 1.0 } else { 0.0 };
                    let (rect, response) = ui
                        .allocate_exact_size(vec2(ui.available_width(), TREE_ROW), Sense::click());
                    ui::paint_row(ui, rect, self.settings_page == page, response.hovered());
                    let x = rect.left() + 8.0 + depth * 18.0;
                    if page.is_group() {
                        let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);
                        let chevron = Rect::from_center_size(
                            pos2(x + 8.0, rect.center().y),
                            vec2(18.0, 18.0),
                        );
                        ui::paint_chevron(ui, chevron.center(), open || !filter.is_empty());
                        let toggled = response.clicked()
                            && response
                                .interact_pointer_pos()
                                .is_some_and(|p| chevron.contains(p));
                        if toggled {
                            ui.data_mut(|d| d.insert_temp(open_id, !open));
                        } else if response.clicked() {
                            select = Some(page);
                            ui.data_mut(|d| d.insert_temp(open_id, true));
                        }
                    } else if response.clicked() {
                        select = Some(page);
                    }
                    let modified = self
                        .settings_draft
                        .page_differs(&self.settings_original, page)
                        || page
                            .children()
                            .any(|c| self.settings_draft.page_differs(&self.settings_original, c));
                    let color = if page.matches(&filter) || filter.is_empty() {
                        t.text
                    } else {
                        t.text_secondary
                    };
                    // A modified page is in the link color, as IntelliJ marks it.
                    let color = if modified { t.link } else { color };
                    ui::paint_text(
                        ui,
                        rect,
                        x + 20.0,
                        page.title(),
                        color,
                        FontId::proportional(13.0),
                    );
                }
            });
        if let Some(page) = select {
            self.settings_page = page;
        }
    }

    fn settings_page_view(
        &mut self,
        ui: &mut egui::Ui,
        t: &Theme,
        command: &mut Option<String>,
        builtin_command: &mut Option<(&'static str, &'static str)>,
    ) {
        let page = self.settings_page;
        // Breadcrumb, and the reset of a modified page on the right.
        ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            if let Some(parent) = page.parent() {
                if ui
                    .add(
                        egui::Label::new(
                            RichText::new(parent.title())
                                .font(ui::semibold(14.0))
                                .color(t.text_secondary),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.settings_page = parent;
                }
                ui.label(
                    RichText::new("›")
                        .font(ui::semibold(14.0))
                        .color(t.text_secondary),
                );
            }
            ui.label(RichText::new(page.title()).font(ui::semibold(14.0)));
            if self
                .settings_draft
                .page_differs(&self.settings_original, page)
            {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.link(tr!("reset")).clicked() {
                        let original = self.settings_original.clone();
                        self.settings_draft.reset_page(&original, page);
                    }
                });
            }
        });
        ui.add_space(10.0);
        if page == Page::Plugins {
            self.plugins_page(ui, *t, command);
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("settings_page", page))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match page {
                    Page::AppearanceBehavior | Page::Editor | Page::Languages | Page::Build => {
                        // A group lists its pages, as links.
                        for sub in page.children() {
                            if ui.link(sub.title()).clicked() {
                                self.settings_page = sub;
                            }
                            ui.add_space(4.0);
                        }
                    }
                    Page::Appearance => {
                        form_row(ui, tr!("theme_label"), |ui| {
                            egui::ComboBox::from_id_salt("settings_theme")
                                .width(220.0)
                                .selected_text(
                                    ui::THEMES[self.settings_draft.theme.min(ui::THEMES.len() - 1)],
                                )
                                .show_ui(ui, |ui| {
                                    for (index, name) in ui::THEMES.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.settings_draft.theme,
                                            index,
                                            *name,
                                        );
                                    }
                                });
                        });
                    }
                    Page::LanguageRegion => {
                        form_row(ui, tr!("language_label"), |ui| {
                            egui::ComboBox::from_id_salt("settings_language")
                                .width(220.0)
                                .selected_text(crate::i18n::name(&self.settings_draft.language))
                                .show_ui(ui, |ui| {
                                    for identifier in crate::i18n::identifiers() {
                                        ui.selectable_value(
                                            &mut self.settings_draft.language,
                                            identifier.to_string(),
                                            crate::i18n::name(identifier),
                                        );
                                    }
                                });
                        });
                        note(ui, t, tr!("language_interface_changes_as_soon"));
                    }
                    Page::Keymap => self.keymap_page(ui, t),
                    Page::EditorGeneral => {
                        group(ui, t, tr!("mouse_control"));
                        let mut wheel_zoom = !self.settings_draft.wheel_zoom_off;
                        if ui::checkbox(ui, &mut wheel_zoom, tr!("change_font_size_ctrl_mouse"))
                            .changed()
                        {
                            self.settings_draft.wheel_zoom_off = !wheel_zoom;
                        }
                        ui.add_space(8.0);
                        group(ui, t, tr!("code_display"));
                        let draft = &mut self.settings_draft;
                        for (off, label) in [
                            (&mut draft.inlay_hints_off, tr!("show_inlay_hints")),
                            (&mut draft.sticky_lines_off, tr!("show_sticky_lines")),
                            (&mut draft.breadcrumbs_off, tr!("show_breadcrumbs")),
                            (&mut draft.spellcheck_off, tr!("check_spelling")),
                            (&mut draft.commit_check_off, tr!("check_before_commit")),
                        ] {
                            let mut on = !*off;
                            if ui::checkbox(ui, &mut on, label).changed() {
                                *off = !on;
                            }
                        }
                    }
                    Page::Fonts => self.fonts_page(ui, t),
                    Page::Sync => self.sync_page(ui, t),
                    Page::JavaScript => self.language_server_page(
                        ui,
                        t,
                        "js",
                        tr!("typescript_language_server_server_gives"),
                        ("javascript", "js.install", "js.restart"),
                        builtin_command,
                    ),
                    Page::Rust => self.language_server_page(
                        ui,
                        t,
                        "rs",
                        tr!("rust_analyzer_knows_types_traits"),
                        ("rust", "rust.install", "rust.restart"),
                        builtin_command,
                    ),
                    Page::RunTools => {
                        note(ui, t, tr!("cargo_python_java_node_tsx"));
                        ui.add_space(8.0);
                        egui::Grid::new("settings_run_tools")
                            .num_columns(3)
                            .spacing([28.0, 10.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new(tr!("language_2")).color(t.text_secondary));
                                ui.label(RichText::new(tr!("command_f5")).color(t.text_secondary));
                                ui.label(RichText::new(tr!("status")).color(t.text_secondary));
                                ui.end_row();
                                for tool in &self.run_tools {
                                    ui.label(tool.language);
                                    ui.label(RichText::new(tool.command).monospace());
                                    if tool.found {
                                        ui.label(RichText::new(tr!("found_path")).color(t.success));
                                    } else {
                                        ui.label(RichText::new(tr!("not_found")).color(t.error));
                                    }
                                    ui.end_row();
                                }
                            });
                    }
                    Page::Plugins => {}
                }
            });
    }

    fn keymap_page(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let filter_id = ui.make_persistent_id("keymap_filter");
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        form_row(ui, tr!("keymap_label"), |ui| {
            ui.add_enabled_ui(false, |ui| {
                egui::ComboBox::from_id_salt("settings_keymap")
                    .width(220.0)
                    .selected_text(tr!("intellij_windows"))
                    .show_ui(ui, |_| {});
            });
        });
        ui.add_space(6.0);
        ui::search_field(ui, &mut filter, tr!("search_actions"), 320.0);
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        ui.add_space(8.0);
        let wanted = filter.trim().to_lowercase();
        // The IDE's actions, then those the plugins add with a shortcut.
        let mut rows: Vec<(String, String)> = SHORTCUTS
            .iter()
            .map(|(key, action)| (action.get().to_string(), key.get().to_string()))
            .collect();
        for plugin in &self.builtins {
            for command in plugin.commands() {
                if let Some((label, keys)) = command.label.split_once("  ")
                    && !rows.iter().any(|(_, k)| k == keys.trim())
                {
                    rows.push((label.to_string(), keys.trim().to_string()));
                }
            }
        }
        rows.retain(|(action, keys)| {
            wanted.is_empty()
                || action.to_lowercase().contains(&wanted)
                || keys.to_lowercase().contains(&wanted)
        });
        ui.spacing_mut().item_spacing.y = 0.0;
        for (action, keys) in rows {
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), TREE_ROW), Sense::hover());
            ui::paint_row(ui, rect, false, response.hovered());
            ui::paint_text(
                ui,
                rect,
                rect.left() + 10.0,
                &action,
                t.text,
                FontId::proportional(13.0),
            );
            // The shortcut, right-aligned, as a key.
            let galley =
                ui.painter()
                    .layout_no_wrap(keys, FontId::proportional(11.5), t.text_secondary);
            let key = Rect::from_min_size(
                pos2(
                    rect.right() - galley.size().x - 22.0,
                    rect.center().y - galley.size().y / 2.0 - 3.0,
                ),
                galley.size() + vec2(12.0, 6.0),
            );
            ui.painter().rect(
                key,
                4.0,
                t.hover,
                egui::Stroke::new(1.0_f32, t.border),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .galley(key.min + vec2(6.0, 3.0), galley, t.text_secondary);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn language_server_page(
        &mut self,
        ui: &mut egui::Ui,
        t: &Theme,
        language: &str,
        about: &str,
        (plugin, install, restart): (&'static str, &'static str, &'static str),
        builtin_command: &mut Option<(&'static str, &'static str)>,
    ) {
        note(ui, t, about);
        ui.add_space(10.0);
        let status = self.languages.status(language);
        form_row(ui, tr!("server"), |ui| {
            let shown = if status.is_empty() {
                tr!("not_started_yet_starts_first").to_string()
            } else {
                status
            };
            ui.label(RichText::new(shown).color(t.text_secondary));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(LABEL_WIDTH);
            if ui::button(ui, tr!("install_server")).clicked() {
                *builtin_command = Some((plugin, install));
            }
            if ui::button(ui, tr!("restart_server")).clicked() {
                *builtin_command = Some((plugin, restart));
            }
        });
    }
}
