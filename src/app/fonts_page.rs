//! The Fonts page of the settings: the fonts of the interface and of the code, their size, and
//! the fonts installed from Google Fonts, Fontsource, an address or a file.

use super::{
    Ide,
    settings_page::{form_row, group, note},
};
use crate::{
    fonts::{Category, Entry, Provider, Source, Store},
    ui::{self, Theme},
};
use eframe::egui::{self, FontId, Margin, RichText};
use std::time::Duration;

/// The catalog rows shown at once: the search narrows the rest.
const SHOWN: usize = 60;

/// What the Fonts page remembers between frames.
pub(super) struct State {
    /// The catalog fonts are installed from; None for an address or a file.
    provider: Option<Provider>,
    query: String,
    category: Option<Category>,
    address: String,
    store: Store,
}

impl Default for State {
    fn default() -> Self {
        Self {
            provider: Some(Provider::Google),
            query: String::new(),
            category: None,
            address: String::new(),
            store: Store::default(),
        }
    }
}

/// A list of fonts to choose from: those of Atelier, then the installed ones.
fn font_combo(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut String,
    builtin: &[&str],
    installed: &[String],
) {
    egui::ComboBox::from_id_salt(id)
        .width(220.0)
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for name in builtin
                .iter()
                .copied()
                .chain(installed.iter().map(String::as_str))
            {
                ui.selectable_value(value, name.to_string(), name);
            }
        });
}

impl Ide {
    pub(super) fn fonts_page(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let state = &mut self.fonts_page;
        state.store.poll();
        if state.store.busy() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        let installed = state.store.installed().to_vec();
        let (code, text): (Vec<_>, Vec<_>) = installed
            .iter()
            .partition(|f| f.category == Category::Monospace);
        let code: Vec<String> = code.into_iter().map(|f| f.family.clone()).collect();
        let text: Vec<String> = text.into_iter().map(|f| f.family.clone()).collect();

        let draft = &mut self.settings_draft;
        form_row(ui, tr!("interface_font"), |ui| {
            font_combo(
                ui,
                "settings_interface_font",
                &mut draft.fonts.interface,
                &ui::INTERFACE_FONTS,
                &text,
            );
        });
        form_row(ui, tr!("code_font"), |ui| {
            font_combo(
                ui,
                "settings_code_font",
                &mut draft.fonts.code,
                &ui::CODE_FONTS,
                &code,
            );
        });
        form_row(ui, tr!("size_label"), |ui| {
            ui.add(
                egui::DragValue::new(&mut draft.font_size)
                    .range(8.0..=36.0)
                    .speed(0.2)
                    .max_decimals(0),
            );
        });
        ui.add_space(6.0);
        // The preview, in the font of the editor at the chosen size.
        egui::Frame::new()
            .fill(t.island)
            .stroke(egui::Stroke::new(1.0_f32, t.border))
            .corner_radius(6)
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(tr!("atelier_ide_font_preview_fn"))
                        .font(FontId::monospace(draft.font_size))
                        .color(t.text),
                );
            });
        note(ui, t, tr!("fonts_change_on_apply"));

        group(ui, t, tr!("install_fonts"));
        let state = &mut self.fonts_page;
        form_row(ui, tr!("font_source"), |ui| {
            egui::ComboBox::from_id_salt("settings_font_source")
                .width(220.0)
                .selected_text(
                    state
                        .provider
                        .map_or(tr!("address_or_file"), Provider::name),
                )
                .show_ui(ui, |ui| {
                    for provider in Provider::ALL {
                        ui.selectable_value(&mut state.provider, Some(provider), provider.name());
                    }
                    ui.selectable_value(&mut state.provider, None, tr!("address_or_file"));
                });
        });
        let installed_families: Vec<String> = installed.iter().map(|f| f.family.clone()).collect();
        let mut install: Option<(String, Source)> = None;
        match state.provider {
            Some(provider) => {
                ui.horizontal(|ui| {
                    ui::search_field(ui, &mut state.query, tr!("search_fonts"), 260.0);
                    egui::ComboBox::from_id_salt("settings_font_category")
                        .width(150.0)
                        .selected_text(state.category.map_or(tr!("all_categories"), Category::name))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.category, None, tr!("all_categories"));
                            for category in Category::ALL {
                                ui.selectable_value(
                                    &mut state.category,
                                    Some(category),
                                    category.name(),
                                );
                            }
                        });
                });
                ui.add_space(4.0);
                let query = state.query.to_lowercase();
                let category = state.category;
                let mut retry = false;
                let store = &mut state.store;
                let catalog = store.catalog(provider).cloned();
                match catalog {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr!("loading_catalog"));
                        });
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(t.deleted));
                        retry = ui.button(tr!("retry")).clicked();
                    }
                    Some(Ok(entries)) => {
                        let found: Vec<&Entry> = entries
                            .iter()
                            .filter(|e| category.is_none_or(|c| e.category == c))
                            .filter(|e| {
                                query.is_empty() || e.family.to_lowercase().contains(&query)
                            })
                            .collect();
                        for entry in found.iter().take(SHOWN) {
                            ui.horizontal(|ui| {
                                ui.set_min_height(24.0);
                                ui.label(&entry.family);
                                ui.label(
                                    RichText::new(entry.category.name())
                                        .small()
                                        .color(t.text_secondary),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if store.is_installing(&entry.family) {
                                            ui.spinner();
                                        } else if installed_families.contains(&entry.family) {
                                            ui.label(
                                                RichText::new(tr!("installed"))
                                                    .color(t.text_secondary),
                                            );
                                        } else if ui.button(tr!("install")).clicked() {
                                            install = Some((
                                                entry.family.clone(),
                                                Source::Catalog(provider, (*entry).clone()),
                                            ));
                                        }
                                    },
                                );
                            });
                        }
                        if found.len() > SHOWN {
                            note(
                                ui,
                                t,
                                &trf!("fonts_shown", shown = SHOWN, count = found.len()),
                            );
                        } else if found.is_empty() {
                            note(ui, t, tr!("no_font_found"));
                        }
                    }
                }
                if retry {
                    store.retry(provider);
                }
            }
            None => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.address)
                            .hint_text("https://…/font.ttf, https://…/css?family=…")
                            .desired_width(340.0),
                    );
                    let address = state.address.trim().to_string();
                    if ui
                        .add_enabled(
                            !address.is_empty() && !state.store.is_installing(&address),
                            egui::Button::new(tr!("install")),
                        )
                        .clicked()
                    {
                        install = Some((address.clone(), Source::Address(address)));
                    }
                    if ui.button(tr!("choose_font_file")).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("TrueType / OpenType", &["ttf", "otf", "ttc"])
                            .pick_file()
                    {
                        install = Some((path.display().to_string(), Source::File(path)));
                    }
                    if state.store.busy() {
                        ui.spinner();
                    }
                });
                note(ui, t, tr!("font_address_note"));
            }
        }
        if let Some((label, source)) = install {
            state.store.message = None;
            state.store.install(label, source);
        }
        match &state.store.message {
            Some((family, true)) => {
                ui.label(RichText::new(trf!("font_installed", family = family)).color(t.added));
            }
            Some((error, false)) => {
                ui.add(egui::Label::new(RichText::new(error).color(t.deleted)).wrap());
            }
            None => {}
        }

        group(ui, t, tr!("installed_fonts"));
        if installed.is_empty() {
            note(ui, t, tr!("no_installed_fonts"));
        }
        let mut remove = None;
        for font in &installed {
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                ui.label(&font.family);
                ui.label(
                    RichText::new(format!("{} · {}", font.category.name(), font.origin))
                        .small()
                        .color(t.text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.link(tr!("remove")).clicked() {
                        remove = Some(font.family.clone());
                    }
                });
            });
        }
        if let Some(family) = remove {
            state.store.uninstall(&family);
        }
    }
}
