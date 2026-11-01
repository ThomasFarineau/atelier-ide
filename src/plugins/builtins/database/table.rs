//! The view of a SQL table in the database viewer (PostgreSQL, MySQL): its rows in a grid, by
//! pages, with a `WHERE` and an `ORDER BY`, to change in place (a double click on a cell), add,
//! set to `NULL` and delete, by the primary key of the table; and a console for any statement,
//! also opened on a schema or a whole connection, without a table.

use super::{Load, Slot, error_line, spawn, sql};
use crate::ui;
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

/// Rows a page shows.
const PAGE: u64 = 100;
/// Past this length, a cell shows the start of its value.
const CELL: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Table,
    Console,
}

/// What the view asks of the plugin once drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Close,
    /// A statement of the console may have created or dropped tables: the tree is read again.
    StructureChanged,
}

/// A row being added: for each column, its text, and whether it is `NULL` or left to its
/// default.
struct Insert {
    values: Vec<(String, String, Choice)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Value,
    Null,
    Default,
}

pub struct TableView {
    pub dialect: sql::Dialect,
    pub uri: String,
    /// The schema (PostgreSQL) or database (MySQL); None for a console of the whole server.
    pub schema: Option<String>,
    /// None for a console that belongs to no table.
    pub table: Option<String>,
    mode: Mode,
    filter: String,
    order: String,
    skip: u64,
    page: Option<Load<sql::Page>>,
    columns: Option<Load<Vec<sql::ColumnInfo>>>,
    statement: String,
    result: Option<Load<sql::Grid>>,
    /// The cell being changed: row, column, text.
    cell: Option<(usize, usize, String)>,
    insert: Option<Insert>,
    confirm_delete: Option<usize>,
    /// A change in progress, which reads the page again once done.
    writing: Option<Slot<sql::Page>>,
    message: Option<(String, bool)>,
    /// The statement of the console running creates, drops or alters something.
    structure_pending: bool,
}

impl TableView {
    fn blank(
        dialect: sql::Dialect,
        uri: String,
        schema: Option<String>,
        table: Option<String>,
    ) -> Self {
        Self {
            dialect,
            uri,
            schema,
            table,
            mode: Mode::Table,
            filter: String::new(),
            order: String::new(),
            skip: 0,
            page: None,
            columns: None,
            statement: String::new(),
            result: None,
            cell: None,
            insert: None,
            confirm_delete: None,
            writing: None,
            message: None,
            structure_pending: false,
        }
    }

    /// The rows of a table.
    pub fn table(
        ctx: &egui::Context,
        dialect: sql::Dialect,
        uri: String,
        schema: String,
        table: String,
    ) -> Self {
        let mut view = Self::blank(dialect, uri, Some(schema.clone()), Some(table.clone()));
        view.statement = format!(
            "SELECT * FROM {} LIMIT 100;",
            sql::qualified(dialect, &schema, &table)
        );
        view.run(ctx);
        view
    }

    /// A console, in a schema or on the whole server.
    pub fn console(dialect: sql::Dialect, uri: String, schema: Option<String>) -> Self {
        let mut view = Self::blank(dialect, uri, schema, None);
        view.mode = Mode::Console;
        view
    }

    pub fn title(&self) -> String {
        match (&self.schema, &self.table) {
            (Some(schema), Some(table)) => format!("{schema}.{table}"),
            (Some(schema), None) => format!("{schema} · {}", tr!("database.mode_console")),
            _ => tr!("database.mode_console").to_string(),
        }
    }

    fn poll(&mut self) {
        if let Some(page) = &mut self.page {
            page.poll();
        }
        if let Some(columns) = &mut self.columns {
            columns.poll();
        }
        if let Some(result) = &mut self.result {
            result.poll();
        }
        let done = self
            .writing
            .as_ref()
            .and_then(|slot| slot.lock().ok()?.take());
        if let Some(result) = done {
            self.writing = None;
            match result {
                Ok(page) => {
                    self.page = Some(Load::Ready(page));
                    self.cell = None;
                    self.insert = None;
                    self.message = Some((tr!("database.saved").into(), false));
                }
                Err(error) => self.message = Some((error, true)),
            }
        }
    }

    fn page_job(&self) -> impl FnOnce() -> anyhow::Result<sql::Page> + Send + 'static + use<> {
        let (dialect, uri) = (self.dialect, self.uri.clone());
        let schema = self.schema.clone().unwrap_or_default();
        let table = self.table.clone().unwrap_or_default();
        let (filter, order, skip) = (self.filter.clone(), self.order.clone(), self.skip);
        move || sql::page(dialect, &uri, &schema, &table, &filter, &order, skip, PAGE)
    }

    pub fn run(&mut self, ctx: &egui::Context) {
        if self.table.is_none() {
            return;
        }
        self.page = Some(Load::Loading(spawn(ctx, self.page_job())));
        if !matches!(self.columns, Some(Load::Ready(_))) {
            let (dialect, uri) = (self.dialect, self.uri.clone());
            let (schema, table) = (
                self.schema.clone().unwrap_or_default(),
                self.table.clone().unwrap_or_default(),
            );
            self.columns = Some(Load::Loading(spawn(ctx, move || {
                sql::columns(dialect, &uri, &schema, &table)
            })));
        }
    }

    /// Runs a change, then reads the page again.
    fn write(&mut self, ctx: &egui::Context, statement: String) {
        let (dialect, uri) = (self.dialect, self.uri.clone());
        let read = self.page_job();
        self.message = None;
        self.writing = Some(spawn(ctx, move || {
            sql::query(dialect, &uri, &statement)?;
            read()
        }));
    }

    fn execute(&mut self, ctx: &egui::Context) {
        let upper = self.statement.to_ascii_uppercase();
        self.structure_pending = ["CREATE ", "DROP ", "ALTER ", "RENAME "]
            .iter()
            .any(|w| upper.contains(w));
        let (dialect, uri, statement) = (self.dialect, self.uri.clone(), self.statement.clone());
        let schema = self.schema.clone();
        self.result = Some(Load::Loading(spawn(ctx, move || {
            sql::query_in(dialect, &uri, schema.as_deref(), &statement)
        })));
    }

    fn columns(&self) -> &[sql::ColumnInfo] {
        match &self.columns {
            Some(Load::Ready(columns)) => columns,
            _ => &[],
        }
    }

    /// The table has a primary key: its rows can be changed one by one.
    fn editable(&self) -> bool {
        self.columns().iter().any(|c| c.key)
    }
}

/// A row as a JSON object, for the clipboard.
fn row_json(columns: &[String], row: &[Option<String>]) -> String {
    let object: serde_json::Map<String, serde_json::Value> = columns
        .iter()
        .zip(row)
        .map(|(c, v)| {
            let value = v
                .clone()
                .map_or(serde_json::Value::Null, serde_json::Value::String);
            (c.clone(), value)
        })
        .collect();
    serde_json::to_string_pretty(&object).unwrap_or_default()
}

/// What a click in the grid asks.
enum GridAction {
    Edit(usize, usize),
    SetNull(usize, usize),
    Delete(usize),
}

/// The rows in a grid: columns that can be resized, one line per cell, `NULL` apart. With
/// `edit`, the cell being changed shows a field; `keys` are the columns of the primary key.
fn grid_ui(
    ui: &mut egui::Ui,
    grid: &sql::Grid,
    t: ui::Theme,
    keys: &[String],
    editable: bool,
    mut cell: Option<&mut (usize, usize, String)>,
) -> (Option<GridAction>, Option<bool>) {
    let mut action = None;
    // The field of the cell: Some(true) to save, Some(false) to cancel.
    let mut decided = None;
    if let Some(count) = grid.affected {
        ui.label(RichText::new(trf!("database.rows_affected", count = count)).color(t.success));
        return (None, None);
    }
    if grid.columns.is_empty() {
        ui.label(RichText::new(tr!("database.no_row")).color(t.text_secondary));
        return (None, None);
    }
    let height = ui.available_height();
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .min_scrolled_height(0.0)
            .max_scroll_height(height - 24.0);
        for _ in &grid.columns {
            table = table.column(Column::initial(140.0).at_least(40.0).clip(true));
        }
        table
            .header(22.0, |mut header| {
                for column in &grid.columns {
                    header.col(|ui| {
                        let key = keys.contains(column);
                        let text = RichText::new(column).strong().monospace().size(12.0);
                        let response = ui.label(if key { text.color(t.warning) } else { text });
                        if key {
                            response.on_hover_text(tr!("database.primary_key"));
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, grid.rows.len(), |mut row| {
                    let index = row.index();
                    let values = &grid.rows[index];
                    for (column, value) in values.iter().enumerate() {
                        row.col(|ui| {
                            if let Some((r, c, text)) = cell.as_deref_mut()
                                && *r == index
                                && *c == column
                            {
                                let field = ui.add(
                                    egui::TextEdit::singleline(text)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY),
                                );
                                field.request_focus();
                                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    decided = Some(true);
                                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    decided = Some(false);
                                }
                                return;
                            }
                            let text = match value {
                                Some(v) => {
                                    let shown: String = v.chars().take(CELL).collect();
                                    RichText::new(shown.replace('\n', " "))
                                        .monospace()
                                        .size(12.0)
                                }
                                None => RichText::new("NULL")
                                    .monospace()
                                    .size(12.0)
                                    .italics()
                                    .color(t.text_disabled),
                            };
                            let response = ui.add(
                                egui::Label::new(text)
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            );
                            if editable && response.double_clicked() {
                                action = Some(GridAction::Edit(index, column));
                            }
                            response.context_menu(|ui| {
                                ui::menu_begin(ui);
                                if editable
                                    && ui::menu_item(
                                        ui,
                                        Some("edit"),
                                        tr!("database.edit_value"),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                {
                                    action = Some(GridAction::Edit(index, column));
                                    ui.close();
                                }
                                if editable
                                    && ui::menu_item(
                                        ui,
                                        None,
                                        tr!("database.set_null"),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                {
                                    action = Some(GridAction::SetNull(index, column));
                                    ui.close();
                                }
                                if ui::menu_item(
                                    ui,
                                    Some("copy"),
                                    tr!("database.copy_value"),
                                    None,
                                    false,
                                )
                                .clicked()
                                {
                                    ui.ctx().copy_text(value.clone().unwrap_or_default());
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, tr!("database.copy_row"), None, false)
                                    .clicked()
                                {
                                    ui.ctx()
                                        .copy_text(row_json(&grid.columns, &grid.rows[index]));
                                    ui.close();
                                }
                                if editable {
                                    ui::menu_separator(ui);
                                    if ui::menu_item(
                                        ui,
                                        Some("delete"),
                                        tr!("database.delete_row"),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(GridAction::Delete(index));
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                });
            });
    });
    (action, decided)
}

/// Draws the view. What the plugin is to do then.
pub fn show(ui: &mut egui::Ui, view: &mut TableView) -> Outcome {
    let t = ui::theme(ui.ctx());
    let ctx = ui.ctx().clone();
    view.poll();
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(view.title()).font(ui::semibold(13.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            close = ui::icon_button(ui, "close", tr!("database.close_collection")).clicked();
            if ui::icon_button(ui, "refresh", tr!("database.refresh")).clicked() {
                match view.mode {
                    Mode::Table => view.run(&ctx),
                    Mode::Console => view.execute(&ctx),
                }
            }
            if view.table.is_some()
                && view.mode == Mode::Table
                && ui::icon_button_enabled(
                    ui,
                    "add",
                    view.writing.is_none(),
                    tr!("database.new_row"),
                )
                .clicked()
            {
                let values = view
                    .columns()
                    .iter()
                    .map(|c| {
                        // A column with a default (a serial) is left to it.
                        let choice = if c.default.is_some() {
                            Choice::Default
                        } else {
                            Choice::Value
                        };
                        (c.name.clone(), String::new(), choice)
                    })
                    .collect();
                view.insert = Some(Insert { values });
            }
        });
    });
    if close {
        return Outcome::Close;
    }
    // Table or console; a console of no table has no table mode.
    if view.table.is_some() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (mode, label) in [
                (Mode::Table, tr!("database.mode_table")),
                (Mode::Console, tr!("database.mode_console")),
            ] {
                let chosen = view.mode == mode;
                let text = RichText::new(label).small().color(if chosen {
                    egui::Color32::WHITE
                } else {
                    t.text
                });
                let button = egui::Button::new(text)
                    .fill(if chosen { t.accent } else { t.field })
                    .corner_radius(4.0)
                    .min_size(egui::vec2(0.0, 20.0));
                if ui.add(button).clicked() {
                    view.mode = mode;
                }
            }
        });
    }
    if let Some((text, error)) = view.message.clone() {
        if error {
            error_line(ui, &text);
        } else {
            ui.label(RichText::new(text).small().color(t.success));
        }
    }
    match view.mode {
        Mode::Table => {
            table_ui(ui, view, &ctx, t);
            Outcome::Nothing
        }
        Mode::Console => console_ui(ui, view, &ctx, t),
    }
}

/// The form of a new row.
fn insert_ui(ui: &mut egui::Ui, view: &mut TableView, ctx: &egui::Context, t: ui::Theme) {
    let Some(insert) = &mut view.insert else {
        return;
    };
    ui::section(ui, tr!("database.new_row"));
    let columns = match &view.columns {
        Some(Load::Ready(columns)) => columns.clone(),
        _ => Vec::new(),
    };
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            egui::Grid::new("database_insert")
                .num_columns(3)
                .spacing(egui::vec2(6.0, 4.0))
                .show(ui, |ui| {
                    for (i, (name, text, choice)) in insert.values.iter_mut().enumerate() {
                        let info = columns.get(i);
                        let label = RichText::new(name.as_str()).monospace().size(12.0);
                        let label = if info.is_some_and(|c| c.key) {
                            label.color(t.warning)
                        } else {
                            label
                        };
                        ui.label(label)
                            .on_hover_text(info.map_or(String::new(), |c| c.data_type.clone()));
                        ui.add_enabled(
                            *choice == Choice::Value,
                            egui::TextEdit::singleline(text)
                                .font(egui::TextStyle::Monospace)
                                .hint_text(info.map_or("", |c| c.data_type.as_str()))
                                .desired_width(140.0),
                        );
                        egui::ComboBox::from_id_salt(("database_insert_choice", i))
                            .width(80.0)
                            .selected_text(match choice {
                                Choice::Value => tr!("database.value"),
                                Choice::Null => "NULL",
                                Choice::Default => "DEFAULT",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(choice, Choice::Value, tr!("database.value"));
                                if info.is_none_or(|c| c.nullable) {
                                    ui.selectable_value(choice, Choice::Null, "NULL");
                                }
                                ui.selectable_value(choice, Choice::Default, "DEFAULT");
                            });
                        ui.end_row();
                    }
                });
        });
    let mut add = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            cancel = ui::button(ui, tr!("cancel")).clicked();
            add = ui::primary_button(ui, tr!("database.add")).clicked();
        });
    });
    if cancel {
        view.insert = None;
        return;
    }
    if add {
        let values: Vec<(String, Option<String>)> = insert
            .values
            .iter()
            .filter(|(_, _, choice)| *choice != Choice::Default)
            .map(|(name, text, choice)| {
                (
                    name.clone(),
                    (*choice == Choice::Value).then(|| text.clone()),
                )
            })
            .collect();
        let (schema, table) = (
            view.schema.clone().unwrap_or_default(),
            view.table.clone().unwrap_or_default(),
        );
        let statement = sql::insert_statement(view.dialect, &schema, &table, &values);
        view.write(ctx, statement);
    }
    ui.separator();
}

fn table_ui(ui: &mut egui::Ui, view: &mut TableView, ctx: &egui::Context, t: ui::Theme) {
    let mut run = false;
    for (label, text, hint) in [
        ("WHERE", &mut view.filter, "id > 10 AND name LIKE 'A%'"),
        ("ORDER BY", &mut view.order, "created_at DESC"),
    ] {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .small()
                    .monospace()
                    .color(t.text_secondary),
            );
            let field = ui.add(
                egui::TextEdit::singleline(text)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(hint)
                    .desired_width(ui.available_width() - 30.0),
            );
            run |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if label == "WHERE" {
                run |= ui::icon_button(ui, "run", tr!("database.run_query")).clicked();
            }
        });
    }
    if run {
        view.skip = 0;
        view.run(ctx);
    }
    let shown = match &view.page {
        Some(Load::Ready(page)) => Some((page.grid.rows.len() as u64, page.total)),
        _ => None,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if let Some((count, total)) = shown {
            let text = if count == 0 {
                tr!("database.no_row").to_string()
            } else {
                trf!(
                    "database.range",
                    from = view.skip + 1,
                    to = view.skip + count,
                    total = total
                )
            };
            ui.label(RichText::new(text).small().color(t.text_secondary));
        }
        if matches!(view.columns, Some(Load::Ready(_))) && !view.editable() {
            ui.label(
                RichText::new(tr!("database.read_only"))
                    .small()
                    .color(t.warning),
            )
            .on_hover_text(tr!("database.read_only_why"));
        }
        if view.writing.is_some() {
            ui.add(egui::Spinner::new().size(12.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let more = shown.is_some_and(|(count, total)| view.skip + count < total);
            if ui::icon_button_enabled(ui, "arrowRight", more, tr!("database.next_page")).clicked()
            {
                view.skip += PAGE;
                view.run(ctx);
            }
            if ui::icon_button_enabled(
                ui,
                "arrowLeft",
                view.skip > 0,
                tr!("database.previous_page"),
            )
            .clicked()
            {
                view.skip = view.skip.saturating_sub(PAGE);
                view.run(ctx);
            }
        });
    });
    ui.separator();
    insert_ui(ui, view, ctx, t);
    let page = match &view.page {
        None | Some(Load::Loading(_)) => return loading(ui, t),
        Some(Load::Failed(error)) => return error_line(ui, &error.clone()),
        Some(Load::Ready(page)) => page.grid.clone(),
    };
    let keys: Vec<String> = view
        .columns()
        .iter()
        .filter(|c| c.key)
        .map(|c| c.name.clone())
        .collect();
    let editable = view.editable() && view.writing.is_none();
    let (action, decided) = grid_ui(ui, &page, t, &keys, editable, view.cell.as_mut());
    let (schema, table) = (
        view.schema.clone().unwrap_or_default(),
        view.table.clone().unwrap_or_default(),
    );
    let columns = view.columns().to_vec();
    let dialect = view.dialect;
    let row_ref = |index: usize| sql::RowRef {
        dialect,
        schema: &schema,
        table: &table,
        columns: &columns,
        names: &page.columns,
        row: &page.rows[index],
    };
    match decided {
        Some(true) => {
            if let Some((index, column, text)) = view.cell.clone()
                && let Some(statement) = row_ref(index).update(&page.columns[column], &Some(text))
            {
                view.write(ctx, statement);
            }
        }
        Some(false) => view.cell = None,
        None => {}
    }
    match action {
        Some(GridAction::Edit(index, column)) => {
            let text = page.rows[index][column].clone().unwrap_or_default();
            view.cell = Some((index, column, text));
        }
        Some(GridAction::SetNull(index, column)) => {
            if let Some(statement) = row_ref(index).update(&page.columns[column], &None) {
                view.write(ctx, statement);
            }
        }
        Some(GridAction::Delete(index)) => view.confirm_delete = Some(index),
        None => {}
    }
    // Deleting a row: asked first.
    if let Some(index) = view.confirm_delete
        && index < page.rows.len()
    {
        let mut answer = None;
        super::confirm(
            ctx,
            tr!("database.delete_row"),
            tr!("database.delete_row_question"),
            &mut answer,
        );
        match answer {
            Some(true) => {
                view.confirm_delete = None;
                if let Some(statement) = row_ref(index).delete() {
                    view.write(ctx, statement);
                }
            }
            Some(false) => view.confirm_delete = None,
            None => {}
        }
    }
}

fn console_ui(
    ui: &mut egui::Ui,
    view: &mut TableView,
    ctx: &egui::Context,
    t: ui::Theme,
) -> Outcome {
    let editor = ui.add(
        egui::TextEdit::multiline(&mut view.statement)
            .font(egui::TextStyle::Monospace)
            .code_editor()
            .hint_text("CREATE TABLE …; SELECT …; INSERT …")
            .desired_rows(6)
            .desired_width(f32::INFINITY),
    );
    let shortcut =
        editor.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    let mut run = shortcut;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr!("database.console_hint"))
                .small()
                .color(t.text_secondary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            run |= ui::primary_button(ui, tr!("database.execute")).clicked();
        });
    });
    if run {
        view.execute(ctx);
    }
    // Once a statement that changes the structure has run, the tree is read again.
    let outcome = if view.structure_pending && matches!(view.result, Some(Load::Ready(_))) {
        view.structure_pending = false;
        Outcome::StructureChanged
    } else {
        Outcome::Nothing
    };
    ui.separator();
    match &view.result {
        None => {}
        Some(Load::Loading(_)) => loading(ui, t),
        Some(Load::Failed(error)) => error_line(ui, &error.clone()),
        Some(Load::Ready(grid)) => {
            let grid = grid.clone();
            grid_ui(ui, &grid, t, &[], false, None);
        }
    }
    outcome
}

fn loading(ui: &mut egui::Ui, t: ui::Theme) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0));
        ui.label(RichText::new(tr!("database.loading")).color(t.text_secondary));
    });
}
