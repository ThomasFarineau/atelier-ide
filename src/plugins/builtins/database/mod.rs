//! Database viewer built-in plugin: a tool window on the right with the saved connections and,
//! under each, its databases (or schemas) and their collections (or tables).
//!
//! MongoDB (`mongo.rs`): the documents of a collection, with a filter and a sort, by pages, as
//! colored JSON, to copy, edit, add or delete. PostgreSQL and MySQL (`sql.rs`, `table.rs`): the
//! rows of a table in a grid, with a `WHERE` and an `ORDER BY`, and a console for any statement.
//!
//! The connections are kept for the user, out of the projects (`databases.json` in the IDE's
//! folder), so that no password goes into a repository. Every call to a server runs on a thread
//! of its own: the interface never waits. An error takes one line: a short message, the whole
//! text in its tooltip, a click copies it.

mod create_table;
pub mod mongo;
pub mod sql;
mod table;

use crate::{
    plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow},
    ui,
};
use eframe::egui::{self, Color32, FontId, RichText, Sense, TextFormat, text::LayoutJob, vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "database.explorer",
    title: crate::i18n::Text("database.title"),
    icon: "database/icon",
    dock: Dock::Right,
    shortcut: "",
}];

/// How many documents a page shows by default.
const PAGE: i64 = 50;
/// Past this length, the summary of a document is cut.
const SUMMARY: usize = 140;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    MongoDb,
    PostgreSql,
    MySql,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::MongoDb, Kind::PostgreSql, Kind::MySql];

    pub fn label(self) -> &'static str {
        match self {
            Self::MongoDb => "MongoDB",
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::MongoDb => "database/mongodb",
            Self::PostgreSql => "database/postgresql",
            Self::MySql => "database/mysql",
        }
    }

    pub fn default_uri(self) -> &'static str {
        match self {
            Self::MongoDb => "mongodb://localhost:27017",
            Self::PostgreSql => "postgresql://postgres:password@localhost:5432/postgres",
            Self::MySql => "mysql://root:password@localhost:3306",
        }
    }

    fn schemes(self) -> &'static [&'static str] {
        match self {
            Self::MongoDb => &["mongodb://", "mongodb+srv://"],
            Self::PostgreSql => &["postgresql://", "postgres://"],
            Self::MySql => &["mysql://"],
        }
    }

    /// Whether a connection string is one of this kind.
    pub fn accepts(self, uri: &str) -> bool {
        self.schemes().iter().any(|s| uri.starts_with(s))
    }

    fn dialect(self) -> Option<sql::Dialect> {
        match self {
            Self::MongoDb => None,
            Self::PostgreSql => Some(sql::Dialect::Postgres),
            Self::MySql => Some(sql::Dialect::MySql),
        }
    }

    /// The version of the server.
    fn test(self, uri: &str) -> anyhow::Result<String> {
        match self.dialect() {
            None => mongo::test(uri).map(|v| format!("MongoDB {v}")),
            Some(dialect) => sql::test(dialect, uri),
        }
    }

    /// The databases of a MongoDB or MySQL server, the schemas of a PostgreSQL database.
    fn containers(self, uri: &str) -> anyhow::Result<Vec<String>> {
        match self.dialect() {
            None => mongo::databases(uri),
            Some(dialect) => sql::schemas(dialect, uri),
        }
    }

    /// The collections or tables of one of them.
    fn items(self, uri: &str, container: &str) -> anyhow::Result<Vec<String>> {
        match self.dialect() {
            None => mongo::collections(uri, container),
            Some(dialect) => sql::tables(dialect, uri, container),
        }
    }
}

/// An error of a server in a few words; the whole text goes to the tooltip.
pub fn short_error(full: &str) -> String {
    let lower = full.to_ascii_lowercase();
    let known = [
        (
            &[
                "server selection timeout",
                "connection refused",
                "actively refused",
                "econnrefused",
                "os error 10061",
                "can't connect",
            ][..],
            tr!("database.error_unreachable"),
        ),
        (
            &[
                "authentication failed",
                "password authentication",
                "access denied",
                "auth failed",
                "unauthorized",
            ][..],
            tr!("database.error_authentication"),
        ),
        (
            &[
                "no such host",
                "failed to lookup",
                "name or service not known",
                "nodename nor servname",
                "os error 11001",
                "dns",
            ][..],
            tr!("database.error_host"),
        ),
        (&["timed out", "timeout"][..], tr!("database.error_timeout")),
    ];
    for (needles, message) in known {
        if !message.is_empty() && needles.iter().any(|n| lower.contains(n)) {
            return message.to_string();
        }
    }
    // The first line, without the names of the layers it went through.
    let line = full.lines().next().unwrap_or(full).trim();
    let line = line
        .strip_prefix("Kind: ")
        .map_or(line, |rest| rest.split(", labels:").next().unwrap_or(rest));
    let mut short: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        short.push('…');
    }
    short
}

/// An error on one line that never makes its panel wider: an icon and a short message, the
/// whole text in the tooltip; a click copies it.
pub(crate) fn error_line(ui: &mut egui::Ui, full: &str) {
    let t = ui::theme(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ui::ROW), Sense::click());
    ui::paint_icon(
        ui,
        "error",
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 10.0, rect.center().y),
            vec2(14.0, 14.0),
        ),
    );
    let mut job =
        LayoutJob::simple_singleline(short_error(full), FontId::proportional(12.5), t.error);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 24.0).max(10.0));
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0),
        galley,
        t.error,
    );
    if response.clicked() {
        ui.ctx().copy_text(full.to_string());
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| {
            ui.set_max_width(420.0);
            ui.label(RichText::new(full).monospace().size(11.5));
            ui.label(
                RichText::new(tr!("database.click_to_copy"))
                    .small()
                    .color(t.text_secondary),
            );
        });
}

/// A saved connection.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Connection {
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    pub uri: String,
}

fn store_path() -> PathBuf {
    crate::cli::data_dir().join("databases.json")
}

pub fn load_connections() -> Vec<Connection> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_connections(connections: &[Connection]) -> Result<(), String> {
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(connections).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// The address without its password, to show it.
pub fn hide_password(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_string();
    };
    let Some((credentials, host)) = rest.split_once('@') else {
        return uri.to_string();
    };
    match credentials.split_once(':') {
        Some((user, _)) => format!("{scheme}://{user}:•••@{host}"),
        None => uri.to_string(),
    }
}

/// A result that a thread gives.
type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn spawn<T: Send + 'static>(
    ctx: &egui::Context,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Slot<T> {
    let slot: Slot<T> = Arc::default();
    let (answer, ctx) = (slot.clone(), ctx.clone());
    std::thread::spawn(move || {
        let result = work().map_err(|e| format!("{e:#}"));
        *answer.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        ctx.request_repaint();
    });
    slot
}

/// Something asked of a server.
enum Load<T> {
    Loading(Slot<T>),
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    fn poll(&mut self) {
        let result = match self {
            Self::Loading(slot) => slot.lock().unwrap_or_else(|e| e.into_inner()).take(),
            _ => None,
        };
        if let Some(result) = result {
            *self = match result {
                Ok(value) => Self::Ready(value),
                Err(error) => Self::Failed(error),
            };
        }
    }
}

/// A document being written: an existing one (its index in the page) or a new one.
struct Edit {
    index: Option<usize>,
    text: String,
    error: Option<String>,
}

/// The collection open.
struct View {
    uri: String,
    database: String,
    collection: String,
    filter: String,
    sort: String,
    skip: u64,
    limit: i64,
    page: Option<Load<mongo::Page>>,
    /// The documents unfolded, by their index in the page.
    open: HashSet<usize>,
    edit: Option<Edit>,
    /// A write in progress, which reads the page again once done, and what to say then.
    writing: Option<(Slot<mongo::Page>, &'static str)>,
    message: Option<(String, bool)>,
    confirm_delete: Option<usize>,
}

impl View {
    fn query(&self) -> mongo::Query {
        mongo::Query {
            database: self.database.clone(),
            collection: self.collection.clone(),
            filter: self.filter.clone(),
            sort: self.sort.clone(),
            skip: self.skip,
            limit: self.limit,
        }
    }

    fn run(&mut self, ctx: &egui::Context) {
        let (uri, query) = (self.uri.clone(), self.query());
        self.page = Some(Load::Loading(spawn(ctx, move || mongo::find(&uri, &query))));
        self.open.clear();
    }
}

/// The dialog of a connection: a new one or one to change.
struct Dialog {
    index: Option<usize>,
    kind: Kind,
    name: String,
    uri: String,
    test: Option<Load<String>>,
    error: Option<String>,
}

#[derive(Default)]
pub struct DatabasePlugin {
    loaded: bool,
    connections: Vec<Connection>,
    databases: HashMap<usize, Load<Vec<String>>>,
    collections: HashMap<(usize, String), Load<Vec<String>>>,
    /// The connections and databases unfolded in the tree.
    expanded: HashSet<String>,
    selected: Option<usize>,
    view: Option<View>,
    table: Option<table::TableView>,
    dialog: Option<Dialog>,
    confirm_remove: Option<usize>,
    create_table: Option<create_table::CreateTable>,
    prompt: Option<Prompt>,
    danger: Option<Danger>,
    /// A change asked from the tree (create, drop), of a connection.
    job: Option<(usize, Slot<()>)>,
    /// What the last change from the tree failed with.
    tree_error: Option<String>,
}

/// What a click in the tree asks.
enum TreeAction {
    Open(usize, String, String),
    Console(usize, Option<String>),
    NewContainer(usize),
    NewTable(usize, String),
    NewCollection(usize, String),
    DropContainer(usize, String),
    DropItem(usize, String, String),
    Truncate(usize, String, String),
    Refresh(usize),
}

/// A dialog that asks names: of a new database, schema or collection.
struct Prompt {
    title: &'static str,
    fields: Vec<(&'static str, String)>,
    action: PromptAction,
}

enum PromptAction {
    /// A schema (PostgreSQL) or a database (MySQL, MongoDB with its first collection).
    NewContainer(usize),
    NewCollection(usize, String),
}

/// A change that cannot be undone, asked first.
struct Danger {
    title: String,
    text: String,
    action: TreeAction,
}

fn database_key(connection: usize, database: &str) -> String {
    format!("{connection}/{database}")
}

impl DatabasePlugin {
    fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.connections = load_connections();
            self.loaded = true;
        }
    }

    fn poll(&mut self) {
        let done = self
            .job
            .as_ref()
            .and_then(|(connection, slot)| Some((*connection, slot.lock().ok()?.take()?)));
        if let Some((connection, result)) = done {
            self.job = None;
            self.forget_tree(connection);
            if let Err(error) = result {
                self.tree_error = Some(error);
            }
        }
        for load in self.databases.values_mut() {
            load.poll();
        }
        for load in self.collections.values_mut() {
            load.poll();
        }
        if let Some(dialog) = &mut self.dialog
            && let Some(test) = &mut dialog.test
        {
            test.poll();
        }
        let Some(view) = &mut self.view else { return };
        if let Some(page) = &mut view.page {
            page.poll();
        }
        let done = view
            .writing
            .as_ref()
            .and_then(|(slot, what)| Some((slot.lock().ok()?.take()?, *what)));
        if let Some((result, what)) = done {
            view.writing = None;
            match result {
                Ok(page) => {
                    view.message = Some((what.to_string(), false));
                    view.edit = None;
                    view.page = Some(Load::Ready(page));
                    view.open.clear();
                }
                Err(error) => match &mut view.edit {
                    Some(edit) => edit.error = Some(error),
                    None => view.message = Some((error, true)),
                },
            }
        }
    }

    /// Forgets what was read of one connection: it is read again when shown.
    fn forget_tree(&mut self, connection: usize) {
        self.databases.remove(&connection);
        self.collections.retain(|(c, _), _| *c != connection);
    }

    /// Runs a change of the tree in the background; the connection is read again after it.
    fn run_job(
        &mut self,
        ctx: &egui::Context,
        connection: usize,
        work: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
    ) {
        self.tree_error = None;
        self.job = Some((connection, spawn(ctx, work)));
    }

    /// Runs a statement of a SQL connection from the tree.
    fn run_sql(&mut self, ctx: &egui::Context, connection: usize, statement: String) {
        let Some(c) = self.connections.get(connection) else {
            return;
        };
        let (Some(dialect), uri) = (c.kind.dialect(), c.uri.clone()) else {
            return;
        };
        self.run_job(ctx, connection, move || {
            sql::query(dialect, &uri, &statement).map(|_| ())
        });
    }

    /// What a click in the tree asked.
    fn apply(&mut self, ctx: &egui::Context, action: TreeAction) {
        let Some(connection) = self.connections.get(match &action {
            TreeAction::Open(c, ..)
            | TreeAction::Console(c, _)
            | TreeAction::NewContainer(c)
            | TreeAction::NewTable(c, _)
            | TreeAction::NewCollection(c, _)
            | TreeAction::DropContainer(c, _)
            | TreeAction::DropItem(c, ..)
            | TreeAction::Truncate(c, ..)
            | TreeAction::Refresh(c) => *c,
        }) else {
            return;
        };
        let (kind, uri) = (connection.kind, connection.uri.clone());
        match action {
            TreeAction::Open(c, container, item) => self.open_collection(ctx, c, &container, &item),
            TreeAction::Console(_, schema) => {
                if let Some(dialect) = kind.dialect() {
                    self.view = None;
                    self.table = Some(table::TableView::console(dialect, uri, schema));
                }
            }
            TreeAction::NewContainer(c) => {
                let (title, fields) = match kind {
                    Kind::MongoDb => (
                        tr!("database.new_database"),
                        vec![
                            (tr!("database.database_name"), String::new()),
                            (tr!("database.collection_name"), String::new()),
                        ],
                    ),
                    Kind::PostgreSql => (
                        tr!("database.new_schema"),
                        vec![(tr!("database.schema_name"), String::new())],
                    ),
                    Kind::MySql => (
                        tr!("database.new_database"),
                        vec![(tr!("database.database_name"), String::new())],
                    ),
                };
                self.prompt = Some(Prompt {
                    title,
                    fields,
                    action: PromptAction::NewContainer(c),
                });
            }
            TreeAction::NewTable(c, schema) => {
                if let Some(dialect) = kind.dialect() {
                    self.create_table =
                        Some(create_table::CreateTable::new(c, dialect, uri, schema));
                }
            }
            TreeAction::NewCollection(c, database) => {
                self.prompt = Some(Prompt {
                    title: tr!("database.new_collection"),
                    fields: vec![(tr!("database.collection_name"), String::new())],
                    action: PromptAction::NewCollection(c, database),
                });
            }
            TreeAction::Refresh(c) => self.forget_tree(c),
            // The changes that cannot be undone: asked first.
            TreeAction::DropContainer(_, ref name) => {
                let title = match kind {
                    Kind::PostgreSql => tr!("database.drop_schema"),
                    _ => tr!("database.drop_database"),
                };
                self.danger = Some(Danger {
                    title: title.into(),
                    text: trf!("database.drop_question", name = name),
                    action,
                });
            }
            TreeAction::DropItem(_, ref container, ref item) => {
                let title = match kind {
                    Kind::MongoDb => tr!("database.drop_collection"),
                    _ => tr!("database.drop_table"),
                };
                self.danger = Some(Danger {
                    title: title.into(),
                    text: trf!(
                        "database.drop_question",
                        name = format!("{container}.{item}")
                    ),
                    action,
                });
            }
            TreeAction::Truncate(_, ref container, ref item) => {
                self.danger = Some(Danger {
                    title: tr!("database.truncate_table").into(),
                    text: trf!(
                        "database.truncate_question",
                        name = format!("{container}.{item}")
                    ),
                    action,
                });
            }
        }
    }

    /// Runs a change confirmed by the user.
    fn run_danger(&mut self, ctx: &egui::Context, action: TreeAction) {
        let Some(connection) = self.connections.get(match &action {
            TreeAction::DropContainer(c, _)
            | TreeAction::DropItem(c, ..)
            | TreeAction::Truncate(c, ..) => *c,
            _ => return,
        }) else {
            return;
        };
        let (kind, uri) = (connection.kind, connection.uri.clone());
        match (action, kind.dialect()) {
            (TreeAction::DropContainer(c, name), None) => {
                self.close_views(&uri, Some(&name), None);
                self.run_job(ctx, c, move || mongo::drop_database(&uri, &name));
            }
            (TreeAction::DropContainer(c, name), Some(dialect)) => {
                self.close_views(&uri, Some(&name), None);
                self.run_sql(ctx, c, sql::drop_schema_statement(dialect, &name));
            }
            (TreeAction::DropItem(c, container, item), None) => {
                self.close_views(&uri, Some(&container), Some(&item));
                self.run_job(ctx, c, move || {
                    mongo::drop_collection(&uri, &container, &item)
                });
            }
            (TreeAction::DropItem(c, container, item), Some(dialect)) => {
                self.close_views(&uri, Some(&container), Some(&item));
                let statement =
                    format!("DROP TABLE {}", sql::qualified(dialect, &container, &item));
                self.run_sql(ctx, c, statement);
            }
            (TreeAction::Truncate(c, container, item), Some(dialect)) => {
                let statement = format!(
                    "TRUNCATE TABLE {}",
                    sql::qualified(dialect, &container, &item)
                );
                self.run_sql(ctx, c, statement);
                if let Some(table) = &mut self.table {
                    table.run(ctx);
                }
            }
            _ => {}
        }
    }

    /// Closes the views of what is dropped.
    fn close_views(&mut self, uri: &str, container: Option<&str>, item: Option<&str>) {
        let matches = |u: &str, c: &str, i: Option<&str>| {
            u == uri && container.is_none_or(|x| x == c) && item.is_none_or(|x| Some(x) == i)
        };
        if self
            .view
            .as_ref()
            .is_some_and(|v| matches(&v.uri, &v.database, Some(&v.collection)))
        {
            self.view = None;
        }
        if self.table.as_ref().is_some_and(|v| {
            matches(
                &v.uri,
                v.schema.as_deref().unwrap_or_default(),
                v.table.as_deref(),
            )
        }) {
            self.table = None;
        }
    }

    /// Forgets what was read of the servers: it is read again when shown.
    fn refresh(&mut self, ctx: &egui::Context) {
        self.databases.clear();
        self.collections.clear();
        if let Some(view) = &mut self.view {
            view.run(ctx);
        }
        if let Some(table) = &mut self.table {
            table.run(ctx);
        }
    }

    fn open_collection(
        &mut self,
        ctx: &egui::Context,
        connection: usize,
        database: &str,
        collection: &str,
    ) {
        let Some((uri, kind)) = self
            .connections
            .get(connection)
            .map(|c| (c.uri.clone(), c.kind))
        else {
            return;
        };
        if let Some(dialect) = kind.dialect() {
            self.view = None;
            self.table = Some(table::TableView::table(
                ctx,
                dialect,
                uri,
                database.to_string(),
                collection.to_string(),
            ));
            return;
        }
        self.table = None;
        let mut view = View {
            uri,
            database: database.to_string(),
            collection: collection.to_string(),
            filter: String::new(),
            sort: String::new(),
            skip: 0,
            limit: PAGE,
            page: None,
            open: HashSet::new(),
            edit: None,
            writing: None,
            message: None,
            confirm_delete: None,
        };
        view.run(ctx);
        self.view = Some(view);
    }

    // ---- The tree ---------------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui::icon_button(ui, "add", tr!("database.new_connection")).clicked() {
                self.dialog = Some(Dialog {
                    index: None,
                    kind: Kind::MongoDb,
                    name: String::new(),
                    uri: Kind::MongoDb.default_uri().into(),
                    test: None,
                    error: None,
                });
            }
            if ui::icon_button(ui, "refresh", tr!("database.refresh")).clicked() {
                self.refresh(ui.ctx());
            }
            let selected = self.selected.filter(|&i| i < self.connections.len());
            let sql_selected = selected.filter(|&i| self.connections[i].kind.dialect().is_some());
            if ui::icon_button_enabled(
                ui,
                "terminal",
                sql_selected.is_some(),
                tr!("database.open_console"),
            )
            .clicked()
                && let Some(index) = sql_selected
            {
                self.apply(ui.ctx(), TreeAction::Console(index, None));
            }
            if ui::icon_button_enabled(
                ui,
                "edit",
                selected.is_some(),
                tr!("database.edit_connection"),
            )
            .clicked()
                && let Some(index) = selected
            {
                self.edit_connection(index);
            }
            if ui::icon_button_enabled(
                ui,
                "remove",
                selected.is_some(),
                tr!("database.remove_connection"),
            )
            .clicked()
            {
                self.confirm_remove = selected;
            }
        });
    }

    fn edit_connection(&mut self, index: usize) {
        let connection = &self.connections[index];
        self.dialog = Some(Dialog {
            index: Some(index),
            kind: connection.kind,
            name: connection.name.clone(),
            uri: connection.uri.clone(),
            test: None,
            error: None,
        });
    }

    fn tree(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui.ctx());
        if self.connections.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tr!("database.no_connection")).color(t.text_secondary));
                ui.add_space(6.0);
                if ui::button(ui, tr!("database.new_connection")).clicked() {
                    self.dialog = Some(Dialog {
                        index: None,
                        kind: Kind::MongoDb,
                        name: String::new(),
                        uri: Kind::MongoDb.default_uri().into(),
                        test: None,
                        error: None,
                    });
                }
            });
            return;
        }
        let ctx = ui.ctx().clone();
        let mut action: Option<TreeAction> = None;
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(
                    RichText::new(tr!("database.working"))
                        .small()
                        .color(t.text_secondary),
                );
            });
        }
        if let Some(error) = self.tree_error.clone() {
            error_line(ui, &error);
        }
        for index in 0..self.connections.len() {
            let key = index.to_string();
            let open = self.expanded.contains(&key);
            let selected = self.selected == Some(index);
            let (rect, response) = ui::row(ui, selected);
            ui::paint_chevron(ui, egui::pos2(rect.left() + 14.0, rect.center().y), open);
            ui::paint_icon(
                ui,
                self.connections[index].kind.icon(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 32.0, rect.center().y),
                    vec2(16.0, 16.0),
                ),
            );
            let name = &self.connections[index].name;
            let right = ui::paint_text(
                ui,
                rect,
                rect.left() + 44.0,
                name,
                t.text,
                FontId::proportional(13.0),
            );
            let address = hide_password(&self.connections[index].uri);
            ui::paint_text(
                ui,
                rect,
                right + 8.0,
                &address,
                t.text_secondary,
                FontId::proportional(11.5),
            );
            if response.clicked() {
                self.selected = Some(index);
                self.toggle(&key, open);
            }
            if response.double_clicked() {
                self.edit_connection(index);
            }
            let kind = self.connections[index].kind;
            response.context_menu(|ui| {
                ui::menu_begin(ui);
                let new_container = match kind {
                    Kind::PostgreSql => tr!("database.new_schema"),
                    _ => tr!("database.new_database"),
                };
                if ui::menu_item(ui, Some("add"), new_container, None, false).clicked() {
                    action = Some(TreeAction::NewContainer(index));
                    ui.close();
                }
                if kind.dialect().is_some()
                    && ui::menu_item(
                        ui,
                        Some("terminal"),
                        tr!("database.open_console"),
                        None,
                        false,
                    )
                    .clicked()
                {
                    action = Some(TreeAction::Console(index, None));
                    ui.close();
                }
                ui::menu_separator(ui);
                if ui::menu_item(
                    ui,
                    Some("edit"),
                    tr!("database.edit_connection"),
                    None,
                    false,
                )
                .clicked()
                {
                    self.edit_connection(index);
                    ui.close();
                }
                if ui::menu_item(ui, Some("refresh"), tr!("database.refresh"), None, false)
                    .clicked()
                {
                    self.databases.remove(&index);
                    self.collections.retain(|(c, _), _| *c != index);
                    ui.close();
                }
                if ui::menu_item(
                    ui,
                    Some("remove"),
                    tr!("database.remove_connection"),
                    None,
                    false,
                )
                .clicked()
                {
                    self.confirm_remove = Some(index);
                    ui.close();
                }
            });
            if !self.expanded.contains(&key) {
                continue;
            }
            let uri = self.connections[index].uri.clone();
            let databases = self.databases.entry(index).or_insert_with(|| {
                let uri = uri.clone();
                Load::Loading(spawn(&ctx, move || kind.containers(&uri)))
            });
            let names = match databases {
                Load::Loading(_) => {
                    status_row(ui, 1, tr!("database.loading"), t.text_secondary);
                    continue;
                }
                Load::Failed(error) => {
                    let error = error.clone();
                    indented(ui, 1, |ui| error_line(ui, &error));
                    continue;
                }
                Load::Ready(names) => names.clone(),
            };
            for database in names {
                let key = database_key(index, &database);
                let open = self.expanded.contains(&key);
                let (rect, response) = ui::row(ui, false);
                let x = rect.left() + 20.0;
                ui::paint_chevron(ui, egui::pos2(x + 14.0, rect.center().y), open);
                ui::paint_icon(
                    ui,
                    "database/icon",
                    egui::Rect::from_center_size(
                        egui::pos2(x + 32.0, rect.center().y),
                        vec2(16.0, 16.0),
                    ),
                );
                ui::paint_text(
                    ui,
                    rect,
                    x + 44.0,
                    &database,
                    t.text,
                    FontId::proportional(13.0),
                );
                if response.clicked() {
                    self.toggle(&key, open);
                }
                response.context_menu(|ui| {
                    ui::menu_begin(ui);
                    let (new_item, drop) = match kind {
                        Kind::MongoDb => (
                            tr!("database.new_collection"),
                            tr!("database.drop_database"),
                        ),
                        Kind::PostgreSql => {
                            (tr!("database.new_table"), tr!("database.drop_schema"))
                        }
                        Kind::MySql => (tr!("database.new_table"), tr!("database.drop_database")),
                    };
                    if ui::menu_item(ui, Some("add"), new_item, None, false).clicked() {
                        action = Some(match kind {
                            Kind::MongoDb => TreeAction::NewCollection(index, database.clone()),
                            _ => TreeAction::NewTable(index, database.clone()),
                        });
                        ui.close();
                    }
                    if kind.dialect().is_some()
                        && ui::menu_item(
                            ui,
                            Some("terminal"),
                            tr!("database.open_console"),
                            None,
                            false,
                        )
                        .clicked()
                    {
                        action = Some(TreeAction::Console(index, Some(database.clone())));
                        ui.close();
                    }
                    if ui::menu_item(ui, Some("refresh"), tr!("database.refresh"), None, false)
                        .clicked()
                    {
                        action = Some(TreeAction::Refresh(index));
                        ui.close();
                    }
                    ui::menu_separator(ui);
                    if ui::menu_item(ui, Some("delete"), drop, None, false).clicked() {
                        action = Some(TreeAction::DropContainer(index, database.clone()));
                        ui.close();
                    }
                });
                if !self.expanded.contains(&key) {
                    continue;
                }
                let uri = uri.clone();
                let db = database.clone();
                let collections = self
                    .collections
                    .entry((index, database.clone()))
                    .or_insert_with(|| Load::Loading(spawn(&ctx, move || kind.items(&uri, &db))));
                let names = match collections {
                    Load::Loading(_) => {
                        status_row(ui, 2, tr!("database.loading"), t.text_secondary);
                        continue;
                    }
                    Load::Failed(error) => {
                        let error = error.clone();
                        indented(ui, 2, |ui| error_line(ui, &error));
                        continue;
                    }
                    Load::Ready(names) => names.clone(),
                };
                if names.is_empty() {
                    status_row(ui, 2, tr!("database.no_collection"), t.text_secondary);
                }
                for collection in names {
                    let here = uri_of(&self.connections, index);
                    let current = self.view.as_ref().is_some_and(|v| {
                        v.uri == here && v.database == database && v.collection == collection
                    }) || self.table.as_ref().is_some_and(|v| {
                        v.uri == here
                            && v.schema.as_deref() == Some(database.as_str())
                            && v.table.as_deref() == Some(collection.as_str())
                    });
                    let (rect, response) = ui::row(ui, current);
                    let x = rect.left() + 40.0;
                    let item_icon = if kind == Kind::MongoDb {
                        "showAsTree"
                    } else {
                        "database/table"
                    };
                    ui::paint_icon(
                        ui,
                        item_icon,
                        egui::Rect::from_center_size(
                            egui::pos2(x + 32.0, rect.center().y),
                            vec2(16.0, 16.0),
                        ),
                    );
                    ui::paint_text(
                        ui,
                        rect,
                        x + 44.0,
                        &collection,
                        t.text,
                        FontId::proportional(13.0),
                    );
                    if response.clicked() {
                        action = Some(TreeAction::Open(
                            index,
                            database.clone(),
                            collection.clone(),
                        ));
                    }
                    response.context_menu(|ui| {
                        ui::menu_begin(ui);
                        if ui::menu_item(ui, None, tr!("database.open"), None, false).clicked() {
                            action = Some(TreeAction::Open(
                                index,
                                database.clone(),
                                collection.clone(),
                            ));
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("copy"), tr!("database.copy_name"), None, false)
                            .clicked()
                        {
                            ui.ctx().copy_text(collection.clone());
                            ui.close();
                        }
                        ui::menu_separator(ui);
                        if kind.dialect().is_some()
                            && ui::menu_item(ui, None, tr!("database.truncate_table"), None, false)
                                .clicked()
                        {
                            action = Some(TreeAction::Truncate(
                                index,
                                database.clone(),
                                collection.clone(),
                            ));
                            ui.close();
                        }
                        let drop = if kind == Kind::MongoDb {
                            tr!("database.drop_collection")
                        } else {
                            tr!("database.drop_table")
                        };
                        if ui::menu_item(ui, Some("delete"), drop, None, false).clicked() {
                            action = Some(TreeAction::DropItem(
                                index,
                                database.clone(),
                                collection.clone(),
                            ));
                            ui.close();
                        }
                    });
                }
            }
        }
        if let Some(action) = action {
            self.apply(&ctx, action);
        }
    }

    fn toggle(&mut self, key: &str, open: bool) {
        if open {
            self.expanded.remove(key);
        } else {
            self.expanded.insert(key.to_string());
        }
    }

    // ---- The collection ---------------------------------------------------------------------

    fn view_ui(&mut self, ui: &mut egui::Ui) {
        let t = ui::theme(ui.ctx());
        let ctx = ui.ctx().clone();
        let Some(view) = &mut self.view else { return };
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}.{}", view.database, view.collection))
                    .font(ui::semibold(13.0)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                close = ui::icon_button(ui, "close", tr!("database.close_collection")).clicked();
                if ui::icon_button(ui, "add", tr!("database.new_document")).clicked() {
                    view.edit = Some(Edit {
                        index: None,
                        text: "{\n  \n}".into(),
                        error: None,
                    });
                }
                if ui::icon_button(ui, "refresh", tr!("database.refresh")).clicked() {
                    view.run(&ctx);
                }
            });
        });
        if close {
            self.view = None;
            return;
        }
        // The filter and the sort.
        let mut run = false;
        ui.horizontal(|ui| {
            let width = ui.available_width() - 30.0;
            let filter = ui.add(
                egui::TextEdit::singleline(&mut view.filter)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("{ \"field\": value }")
                    .desired_width(width),
            );
            run |= filter.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            run |= ui::icon_button(ui, "run", tr!("database.run_query")).clicked();
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tr!("database.sort"))
                    .small()
                    .color(t.text_secondary),
            );
            let sort = ui.add(
                egui::TextEdit::singleline(&mut view.sort)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("{ \"_id\": -1 }")
                    .desired_width(ui.available_width()),
            );
            run |= sort.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        if run {
            view.skip = 0;
            view.run(&ctx);
        }
        // The page.
        let total = match &view.page {
            Some(Load::Ready(page)) => Some((page.documents.len() as u64, page.total)),
            _ => None,
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            match total {
                Some((shown, total)) if shown > 0 => {
                    ui.label(
                        RichText::new(trf!(
                            "database.range",
                            from = view.skip + 1,
                            to = view.skip + shown,
                            total = total
                        ))
                        .small()
                        .color(t.text_secondary),
                    );
                }
                Some(_) => {
                    ui.label(
                        RichText::new(tr!("database.no_document"))
                            .small()
                            .color(t.text_secondary),
                    );
                }
                None => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let more = total.is_some_and(|(shown, total)| view.skip + shown < total);
                if ui::icon_button_enabled(ui, "arrowRight", more, tr!("database.next_page"))
                    .clicked()
                {
                    view.skip += view.limit as u64;
                    view.run(&ctx);
                }
                if ui::icon_button_enabled(
                    ui,
                    "arrowLeft",
                    view.skip > 0,
                    tr!("database.previous_page"),
                )
                .clicked()
                {
                    view.skip = view.skip.saturating_sub(view.limit as u64);
                    view.run(&ctx);
                }
            });
        });
        if let Some((text, error)) = view.message.clone() {
            if error {
                error_line(ui, &text);
            } else {
                ui.label(RichText::new(text).small().color(t.success));
            }
        }
        ui.separator();
        // A document being written.
        if view.edit.is_some() {
            edit_ui(ui, view, &ctx);
            return;
        }
        let documents = match &view.page {
            None | Some(Load::Loading(_)) => {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(RichText::new(tr!("database.loading")).color(t.text_secondary));
                });
                return;
            }
            Some(Load::Failed(error)) => {
                let error = error.clone();
                error_line(ui, &error);
                return;
            }
            Some(Load::Ready(page)) => page.documents.clone(),
        };
        let mut edit = None;
        let mut delete = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, document) in documents.iter().enumerate() {
                    let open = view.open.contains(&index);
                    let (rect, response) = ui::row(ui, false);
                    ui::paint_chevron(ui, egui::pos2(rect.left() + 14.0, rect.center().y), open);
                    ui::paint_text(
                        ui,
                        rect,
                        rect.left() + 26.0,
                        &summary(document),
                        t.text,
                        FontId::monospace(12.0),
                    );
                    if response.clicked() {
                        if open {
                            view.open.remove(&index);
                        } else {
                            view.open.insert(index);
                        }
                    }
                    response.context_menu(|ui| {
                        ui::menu_begin(ui);
                        if ui::menu_item(
                            ui,
                            Some("edit"),
                            tr!("database.edit_document"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            edit = Some(index);
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("copy"), tr!("database.copy_json"), None, false)
                            .clicked()
                        {
                            ui.ctx().copy_text(pretty(document));
                            ui.close();
                        }
                        if ui::menu_item(
                            ui,
                            Some("delete"),
                            tr!("database.delete_document"),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            delete = Some(index);
                            ui.close();
                        }
                    });
                    if !open {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(26.0);
                        ui.vertical(|ui| {
                            let job = json_job(&pretty(document), t.dark, ui.available_width());
                            ui.label(job);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                if ui::icon_button(ui, "edit", tr!("database.edit_document"))
                                    .clicked()
                                {
                                    edit = Some(index);
                                }
                                if ui::icon_button(ui, "copy", tr!("database.copy_json")).clicked()
                                {
                                    ui.ctx().copy_text(pretty(document));
                                }
                                if ui::icon_button(ui, "delete", tr!("database.delete_document"))
                                    .clicked()
                                {
                                    delete = Some(index);
                                }
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            });
        if let Some(index) = edit {
            view.edit = Some(Edit {
                index: Some(index),
                text: pretty(&documents[index]),
                error: None,
            });
        }
        if delete.is_some() {
            view.confirm_delete = delete;
        }
    }

    // ---- Dialogs ----------------------------------------------------------------------------

    /// The dialogs of the tree: the names of something new, a change to confirm, a new table.
    fn tree_dialogs(&mut self, ctx: &egui::Context) {
        let t = ui::theme(ctx);
        if let Some(prompt) = &mut self.prompt {
            let mut answer = None;
            egui::Window::new(prompt.title)
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(360.0);
                    ui::dialog_title(ui, prompt.title);
                    for (i, (label, value)) in prompt.fields.iter_mut().enumerate() {
                        ui.label(RichText::new(*label).small().color(t.text_secondary));
                        let field =
                            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
                        if i == 0 && !field.has_focus() && value.is_empty() {
                            field.request_focus();
                        }
                    }
                    let (ok, cancel) = ui::dialog_buttons(ui, tr!("database.create"));
                    if ok {
                        answer = Some(true);
                    } else if cancel {
                        answer = Some(false);
                    }
                });
            match answer {
                Some(true) => {
                    let values: Vec<String> = prompt
                        .fields
                        .iter()
                        .map(|(_, v)| v.trim().to_string())
                        .collect();
                    if values.iter().all(|v| !v.is_empty())
                        && let Some(prompt) = self.prompt.take()
                    {
                        self.create(ctx, prompt.action, values);
                    }
                }
                Some(false) => self.prompt = None,
                None => {}
            }
        }
        if let Some(danger) = &self.danger {
            let mut answer = None;
            let (title, text) = (danger.title.clone(), danger.text.clone());
            confirm(ctx, &title, &text, &mut answer);
            match answer {
                Some(true) => {
                    if let Some(danger) = self.danger.take() {
                        self.run_danger(ctx, danger.action);
                    }
                }
                Some(false) => self.danger = None,
                None => {}
            }
        }
        if let Some(dialog) = &mut self.create_table {
            match create_table::show(ctx, dialog) {
                create_table::Done::Open => {}
                create_table::Done::Cancelled => self.create_table = None,
                create_table::Done::Created(name) => {
                    let (connection, schema) = (dialog.connection, dialog.schema.clone());
                    self.create_table = None;
                    self.forget_tree(connection);
                    self.expanded.insert(database_key(connection, &schema));
                    self.open_collection(ctx, connection, &schema, &name);
                }
            }
        }
    }

    /// Creates what a dialog of names asked.
    fn create(&mut self, ctx: &egui::Context, action: PromptAction, values: Vec<String>) {
        let connection = match &action {
            PromptAction::NewContainer(c) | PromptAction::NewCollection(c, _) => *c,
        };
        let Some(c) = self.connections.get(connection) else {
            return;
        };
        let (kind, uri) = (c.kind, c.uri.clone());
        self.expanded.insert(connection.to_string());
        match (action, kind.dialect()) {
            (PromptAction::NewContainer(_), None) => {
                let (database, collection) = (values[0].clone(), values[1].clone());
                self.expanded.insert(database_key(connection, &database));
                self.run_job(ctx, connection, move || {
                    mongo::create_collection(&uri, &database, &collection)
                });
            }
            (PromptAction::NewContainer(_), Some(dialect)) => {
                self.run_sql(
                    ctx,
                    connection,
                    sql::create_schema_statement(dialect, &values[0]),
                );
            }
            (PromptAction::NewCollection(_, database), _) => {
                let collection = values[0].clone();
                self.expanded.insert(database_key(connection, &database));
                self.run_job(ctx, connection, move || {
                    mongo::create_collection(&uri, &database, &collection)
                });
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.tree_dialogs(ctx);
        let t = ui::theme(ctx);
        // The connection.
        let mut result = None;
        if let Some(dialog) = &mut self.dialog {
            let title = if dialog.index.is_some() {
                tr!("database.edit_connection")
            } else {
                tr!("database.new_connection")
            };
            egui::Window::new(title)
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui::dialog_title(ui, title);
                    ui.label(
                        RichText::new(tr!("database.kind"))
                            .small()
                            .color(t.text_secondary),
                    );
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for kind in Kind::ALL {
                            let chosen = dialog.kind == kind;
                            let text = RichText::new(kind.label()).color(if chosen {
                                Color32::WHITE
                            } else {
                                t.text
                            });
                            let button =
                                egui::Button::image_and_text(ui::icon(ui, kind.icon()), text)
                                    .fill(if chosen { t.accent } else { t.field })
                                    .corner_radius(4.0)
                                    .min_size(vec2(0.0, 24.0));
                            if ui.add(button).clicked() && !chosen {
                                let example = dialog.uri.trim().is_empty()
                                    || Kind::ALL.iter().any(|k| dialog.uri == k.default_uri());
                                if example {
                                    dialog.uri = kind.default_uri().into();
                                }
                                dialog.kind = kind;
                                dialog.test = None;
                                dialog.error = None;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(tr!("database.name"))
                            .small()
                            .color(t.text_secondary),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .hint_text("Local")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(tr!("database.uri"))
                            .small()
                            .color(t.text_secondary),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.uri)
                            .font(egui::TextStyle::Monospace)
                            .hint_text(dialog.kind.default_uri())
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui::button(ui, tr!("database.test_connection")).clicked() {
                            let uri = dialog.uri.trim().to_string();
                            let kind = dialog.kind;
                            mongo::forget(&uri);
                            dialog.test = Some(Load::Loading(spawn(ctx, move || kind.test(&uri))));
                        }
                        match &dialog.test {
                            Some(Load::Loading(_)) => {
                                ui.add(egui::Spinner::new().size(14.0));
                            }
                            Some(Load::Ready(version)) => {
                                ui.label(
                                    RichText::new(trf!("database.connected", version = version))
                                        .color(t.success),
                                );
                            }
                            _ => {}
                        }
                    });
                    // An error takes one line under the button: the dialog keeps its width.
                    if let Some(Load::Failed(error)) = &dialog.test {
                        let error = error.clone();
                        error_line(ui, &error);
                    }
                    if let Some(error) = dialog.error.clone() {
                        error_line(ui, &error);
                    }
                    ui.label(
                        RichText::new(tr!("database.stored_for_user"))
                            .small()
                            .color(t.text_secondary),
                    );
                    let (ok, cancel) = ui::dialog_buttons(ui, tr!("database.save"));
                    if ok {
                        result = Some(true);
                    } else if cancel {
                        result = Some(false);
                    }
                });
        }
        match result {
            Some(true) => self.save_dialog(),
            Some(false) => self.dialog = None,
            None => {}
        }
        // Removing a connection.
        if let Some(index) = self.confirm_remove {
            let name = self
                .connections
                .get(index)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let mut answer = None;
            confirm(
                ctx,
                tr!("database.remove_connection"),
                &trf!("database.remove_question", name = name),
                &mut answer,
            );
            match answer {
                Some(true) => {
                    if index < self.connections.len() {
                        let removed = self.connections.remove(index);
                        mongo::forget(&removed.uri);
                        let _ = save_connections(&self.connections);
                        self.databases.clear();
                        self.collections.clear();
                        self.expanded.clear();
                        self.selected = None;
                        if self.view.as_ref().is_some_and(|v| v.uri == removed.uri) {
                            self.view = None;
                        }
                        if self.table.as_ref().is_some_and(|v| v.uri == removed.uri) {
                            self.table = None;
                        }
                    }
                    self.confirm_remove = None;
                }
                Some(false) => self.confirm_remove = None,
                None => {}
            }
        }
        // Deleting a document.
        let Some(view) = &mut self.view else { return };
        let Some(index) = view.confirm_delete else {
            return;
        };
        let mut answer = None;
        confirm(
            ctx,
            tr!("database.delete_document"),
            tr!("database.delete_question"),
            &mut answer,
        );
        match answer {
            Some(true) => {
                view.confirm_delete = None;
                if let Some(Load::Ready(page)) = &view.page
                    && let Some(document) = page.documents.get(index).cloned()
                {
                    let (uri, query) = (view.uri.clone(), view.query());
                    let slot = spawn(ctx, move || {
                        mongo::delete(&uri, &query.database, &query.collection, &document)?;
                        mongo::find(&uri, &query)
                    });
                    view.writing = Some((slot, tr!("database.deleted")));
                }
            }
            Some(false) => view.confirm_delete = None,
            None => {}
        }
    }

    fn save_dialog(&mut self) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let uri = dialog.uri.trim().to_string();
        if !dialog.kind.accepts(&uri) {
            dialog.error = Some(match dialog.kind {
                Kind::MongoDb => tr!("database.uri_scheme_mongodb").into(),
                Kind::PostgreSql => tr!("database.uri_scheme_postgresql").into(),
                Kind::MySql => tr!("database.uri_scheme_mysql").into(),
            });
            return;
        }
        let name = if dialog.name.trim().is_empty() {
            hide_password(&uri)
                .split_once("://")
                .map_or(uri.clone(), |(_, rest)| rest.to_string())
        } else {
            dialog.name.trim().to_string()
        };
        let connection = Connection {
            name,
            kind: dialog.kind,
            uri,
        };
        match dialog.index {
            Some(index) if index < self.connections.len() => {
                mongo::forget(&self.connections[index].uri);
                self.connections[index] = connection;
                self.databases.remove(&index);
                self.collections.retain(|(c, _), _| *c != index);
            }
            _ => {
                self.connections.push(connection);
                let index = self.connections.len() - 1;
                self.selected = Some(index);
                self.expanded.insert(index.to_string());
            }
        }
        match save_connections(&self.connections) {
            Ok(()) => self.dialog = None,
            Err(e) => dialog_error(&mut self.dialog, e),
        }
    }
}

fn dialog_error(dialog: &mut Option<Dialog>, error: String) {
    if let Some(dialog) = dialog {
        dialog.error = Some(error);
    }
}

fn uri_of(connections: &[Connection], index: usize) -> String {
    connections
        .get(index)
        .map(|c| c.uri.clone())
        .unwrap_or_default()
}

/// Content of the tree moved to the depth of a node.
fn indented(ui: &mut egui::Ui, depth: usize, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(20.0 * depth as f32 + 16.0);
        ui.vertical(add);
    });
}

/// A line of the tree that is not a node: loading, an error, nothing.
fn status_row(ui: &mut egui::Ui, depth: usize, text: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ui::ROW), Sense::hover());
    ui::paint_text(
        ui,
        rect,
        rect.left() + 20.0 * depth as f32 + 26.0,
        text,
        color,
        FontId::proportional(12.0),
    );
}

/// A dialog that asks yes or no.
pub(crate) fn confirm(ctx: &egui::Context, title: &str, text: &str, answer: &mut Option<bool>) {
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui::dialog_title(ui, title);
            ui.label(text);
            let (ok, cancel) = ui::dialog_buttons(ui, title);
            if ok {
                *answer = Some(true);
            } else if cancel {
                *answer = Some(false);
            }
        });
}

/// The editor of a document: its JSON, saved as a replacement (or a new document).
fn edit_ui(ui: &mut egui::Ui, view: &mut View, ctx: &egui::Context) {
    let Some(edit) = &mut view.edit else { return };
    let title = if edit.index.is_some() {
        tr!("database.edit_document")
    } else {
        tr!("database.new_document")
    };
    ui::section(ui, title);
    let mut save = false;
    let mut cancel = false;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 64.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut edit.text)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(14),
            );
        });
    if let Some(error) = edit.error.clone() {
        error_line(ui, &error);
    }
    ui.horizontal(|ui| {
        let busy = view.writing.is_some();
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            cancel = ui::button(ui, tr!("cancel")).clicked();
            save = !busy && ui::primary_button(ui, tr!("database.save")).clicked();
        });
    });
    if cancel {
        view.edit = None;
        return;
    }
    if !save {
        return;
    }
    // Checked here first: a mistake shows at once.
    if let Err(error) = mongo::document(&edit.text) {
        edit.error = Some(format!("{error:#}"));
        return;
    }
    // The write, then the page read again.
    let (index, text) = (edit.index, edit.text.clone());
    let (uri, query) = (view.uri.clone(), view.query());
    let (slot, done) = match index {
        Some(index) => {
            let Some(Load::Ready(page)) = &view.page else {
                return;
            };
            let Some(shown) = page.documents.get(index).cloned() else {
                return;
            };
            let slot = spawn(ctx, move || {
                mongo::replace(&uri, &query.database, &query.collection, &shown, &text)?;
                mongo::find(&uri, &query)
            });
            (slot, tr!("database.saved"))
        }
        None => {
            let slot = spawn(ctx, move || {
                mongo::insert(&uri, &query.database, &query.collection, &text)?;
                mongo::find(&uri, &query)
            });
            (slot, tr!("database.inserted"))
        }
    };
    view.writing = Some((slot, done));
}

fn pretty(document: &Value) -> String {
    serde_json::to_string_pretty(document).unwrap_or_default()
}

/// One value, short: `ObjectId("…")`, `"text"`, `{…}`, `[3]`.
fn short(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(id)) = map.get("$oid") {
                return format!("ObjectId(\"{id}\")");
            }
            if let Some(date) = map.get("$date") {
                return match date {
                    Value::String(text) => format!("Date(\"{text}\")"),
                    other => format!("Date({other})"),
                };
            }
            "{…}".into()
        }
        Value::Array(items) => format!("[{}]", items.len()),
        other => other.to_string(),
    }
}

/// A document on one line: its first fields.
pub fn summary(document: &Value) -> String {
    let Value::Object(map) = document else {
        return document.to_string();
    };
    let mut out = String::from("{ ");
    for (i, (key, value)) in map.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&short(value));
        if out.chars().count() > SUMMARY {
            break;
        }
    }
    out.push_str(" }");
    if out.chars().count() > SUMMARY {
        out = out.chars().take(SUMMARY).collect::<String>() + "…";
    }
    out
}

/// Pretty JSON in the colors of the editor: keys, strings, numbers, `true`, `false`, `null`.
fn json_job(text: &str, dark: bool, width: f32) -> LayoutJob {
    let (key, string, number, keyword, plain) = if dark {
        (0xC77DBB, 0x6AAB73, 0x2AACB8, 0xCF8E6D, 0xBCBEC4)
    } else {
        (0x871094, 0x067D17, 0x1750EB, 0x0033B3, 0x080808)
    };
    let color = |rgb: u32| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let font = FontId::monospace(12.0);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let push = |job: &mut LayoutJob, piece: &str, rgb: u32| {
        job.append(piece, 0.0, TextFormat::simple(font.clone(), color(rgb)));
    };
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                let is_key = text[i..].trim_start().starts_with(':');
                push(&mut job, &text[start..i], if is_key { key } else { string });
            }
            b'-' | b'0'..=b'9' => {
                while i < bytes.len()
                    && matches!(bytes[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    i += 1;
                }
                push(&mut job, &text[start..i], number);
            }
            b't' | b'f' | b'n' => {
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                push(&mut job, &text[start..i], keyword);
            }
            _ => {
                while i < bytes.len()
                    && !matches!(bytes[i], b'"' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n')
                {
                    i += 1;
                }
                push(&mut job, &text[start..i], plain);
            }
        }
    }
    job
}

impl BuiltinPlugin for DatabasePlugin {
    fn id(&self) -> &'static str {
        "database"
    }
    fn name(&self) -> &'static str {
        tr!("database.title")
    }
    fn description(&self) -> &'static str {
        tr!("database.description")
    }
    fn icon(&self) -> &'static str {
        "database/icon"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }

    fn update(&mut self, _ctx: &egui::Context, _host: &mut dyn Host) {
        self.poll();
    }

    fn tool_window(&mut self, id: &str, ui: &mut egui::Ui, _host: &mut dyn Host) {
        if id != TOOL_WINDOWS[0].id {
            return;
        }
        self.ensure_loaded();
        self.poll();
        ui.set_min_height(ui.available_height());
        self.toolbar(ui);
        ui.separator();
        if self.view.is_some() || self.table.is_some() {
            egui::TopBottomPanel::top("database_tree")
                .resizable(true)
                .default_height((ui.available_height() * 0.35).max(80.0))
                .height_range(60.0..=(ui.available_height() - 120.0).max(80.0))
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.tree(ui));
                });
            ui.add_space(4.0);
            if let Some(table) = &mut self.table {
                match table::show(ui, table) {
                    table::Outcome::Close => self.table = None,
                    table::Outcome::StructureChanged => {
                        let uri = table.uri.clone();
                        if let Some(index) = self.connections.iter().position(|c| c.uri == uri) {
                            self.forget_tree(index);
                        }
                    }
                    table::Outcome::Nothing => {}
                }
            } else {
                self.view_ui(ui);
            }
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.tree(ui));
        }
        self.dialogs(ui.ctx());
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: "database.open",
                label: tr!("database.command_open").into(),
            },
            Command {
                id: "database.new",
                label: tr!("database.command_new").into(),
            },
        ]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "database.open" => host.show_tool_window(&TOOL_WINDOWS[0]),
            "database.new" => {
                self.ensure_loaded();
                host.show_tool_window(&TOOL_WINDOWS[0]);
                self.dialog = Some(Dialog {
                    index: None,
                    kind: Kind::MongoDb,
                    name: String::new(),
                    uri: Kind::MongoDb.default_uri().into(),
                    test: None,
                    error: None,
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/database/tests.rs"]
mod tests;
