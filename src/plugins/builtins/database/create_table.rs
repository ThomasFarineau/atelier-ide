//! The dialog that creates a SQL table: its name, its columns (name, type, primary key,
//! `NOT NULL`, default), and the statement that will run, shown as it is written.

use super::{Load, error_line, spawn, sql};
use crate::ui;
use eframe::egui::{self, RichText, vec2};

pub struct CreateTable {
    pub connection: usize,
    pub dialect: sql::Dialect,
    pub uri: String,
    pub schema: String,
    name: String,
    columns: Vec<sql::NewColumn>,
    running: Option<Load<sql::Grid>>,
}

impl CreateTable {
    pub fn new(connection: usize, dialect: sql::Dialect, uri: String, schema: String) -> Self {
        Self {
            connection,
            dialect,
            uri,
            schema,
            name: String::new(),
            columns: vec![
                sql::identity_column(dialect),
                sql::NewColumn {
                    name: "name".into(),
                    data_type: text_type(dialect).into(),
                    key: false,
                    not_null: false,
                    default: String::new(),
                },
            ],
            running: None,
        }
    }

    fn statement(&self) -> String {
        sql::create_table_statement(self.dialect, &self.schema, self.name.trim(), &self.columns)
    }
}

/// What the dialog ended with.
pub enum Done {
    Open,
    /// Created: the name of the table.
    Created(String),
    Cancelled,
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, dialog: &mut CreateTable) -> Done {
    let t = ui::theme(ctx);
    if let Some(running) = &mut dialog.running {
        running.poll();
        if matches!(running, Load::Ready(_)) {
            return Done::Created(dialog.name.trim().to_string());
        }
    }
    let mut done = Done::Open;
    egui::Window::new(tr!("database.new_table"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(620.0);
            ui::dialog_title(
                ui,
                &format!("{} — {}", tr!("database.new_table"), dialog.schema),
            );
            ui.label(
                RichText::new(tr!("database.name"))
                    .small()
                    .color(t.text_secondary),
            );
            ui.add(
                egui::TextEdit::singleline(&mut dialog.name)
                    .hint_text("users")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr!("database.columns"))
                    .small()
                    .color(t.text_secondary),
            );
            let mut remove = None;
            let types = sql::types(dialog.dialect);
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    egui::Grid::new("database_new_columns")
                        .num_columns(6)
                        .spacing(vec2(6.0, 4.0))
                        .show(ui, |ui| {
                            for header in [
                                tr!("database.name"),
                                tr!("database.type"),
                                tr!("database.primary_key_short"),
                                "NOT NULL",
                                tr!("database.default"),
                                "",
                            ] {
                                ui.label(RichText::new(header).small().color(t.text_secondary));
                            }
                            ui.end_row();
                            for (i, column) in dialog.columns.iter_mut().enumerate() {
                                ui.add(
                                    egui::TextEdit::singleline(&mut column.name)
                                        .desired_width(120.0),
                                );
                                // A type of the list, or any other one typed.
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.add(
                                        egui::TextEdit::singleline(&mut column.data_type)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(150.0),
                                    );
                                    egui::ComboBox::from_id_salt(("database_type", i))
                                        .selected_text("")
                                        .width(18.0)
                                        .show_ui(ui, |ui| {
                                            for kind in types {
                                                if ui
                                                    .selectable_label(
                                                        column.data_type == *kind,
                                                        *kind,
                                                    )
                                                    .clicked()
                                                {
                                                    column.data_type = (*kind).into();
                                                }
                                            }
                                        });
                                });
                                ui.checkbox(&mut column.key, "");
                                ui.checkbox(&mut column.not_null, "");
                                ui.add(
                                    egui::TextEdit::singleline(&mut column.default)
                                        .font(egui::TextStyle::Monospace)
                                        .hint_text("—")
                                        .desired_width(90.0),
                                );
                                if ui::icon_button(ui, "remove", tr!("database.remove_column"))
                                    .clicked()
                                {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        });
                });
            if let Some(i) = remove {
                dialog.columns.remove(i);
            }
            if ui::button(ui, tr!("database.add_column")).clicked() {
                dialog.columns.push(sql::NewColumn {
                    name: String::new(),
                    data_type: text_type(dialog.dialect).into(),
                    key: false,
                    not_null: false,
                    default: String::new(),
                });
            }
            ui.add_space(6.0);
            // The statement, as it will run.
            ui.label(RichText::new("SQL").small().color(t.text_secondary));
            let mut statement = dialog.statement();
            egui::Frame::new()
                .fill(t.field)
                .corner_radius(4.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::TextEdit::multiline(&mut statement)
                            .font(egui::TextStyle::Monospace)
                            .interactive(false)
                            .frame(false)
                            .desired_width(f32::INFINITY),
                    );
                });
            match &dialog.running {
                Some(Load::Loading(_)) => {
                    ui.add(egui::Spinner::new().size(14.0));
                }
                Some(Load::Failed(error)) => {
                    let error = error.clone();
                    error_line(ui, &error);
                }
                _ => {}
            }
            let named = !dialog.name.trim().is_empty()
                && dialog.columns.iter().any(|c| !c.name.trim().is_empty());
            let (ok, cancel) = ui::dialog_buttons(ui, tr!("database.create"));
            if cancel {
                done = Done::Cancelled;
            } else if ok && named && !matches!(dialog.running, Some(Load::Loading(_))) {
                let (dialect, uri) = (dialog.dialect, dialog.uri.clone());
                let statement = dialog.statement();
                dialog.running = Some(Load::Loading(spawn(ctx, move || {
                    sql::query(dialect, &uri, &statement)
                })));
            }
        });
    done
}

/// The type a new column starts with: text.
fn text_type(dialect: sql::Dialect) -> &'static str {
    match dialect {
        sql::Dialect::Postgres => "text",
        sql::Dialect::MySql => "VARCHAR(255)",
    }
}
