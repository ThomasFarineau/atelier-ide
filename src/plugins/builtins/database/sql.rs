//! The SQL drivers of the database viewer: PostgreSQL and MySQL. What the viewer asks of them
//! is the same: the schemas (or databases) of a server, their tables, a page of a table, and
//! any statement typed in its console. Values come back as text, `NULL` as None.
//!
//! Every call runs on a thread of its own (see `mod.rs`).

use anyhow::{Context, Result, anyhow};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect {
    Postgres,
    MySql,
}

/// Rows as text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// For a statement that changes rows: how many.
    pub affected: Option<u64>,
}

/// A page of a table, and how many rows match.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub grid: Grid,
    pub total: u64,
}

/// A name in the quotes of the dialect: `"a""b"`, `` `a``b` ``.
pub fn quote(dialect: Dialect, name: &str) -> String {
    match dialect {
        Dialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        Dialect::MySql => format!("`{}`", name.replace('`', "``")),
    }
}

/// A text between single quotes, for a literal. MySQL reads backslashes as escapes,
/// PostgreSQL does not.
pub fn literal(dialect: Dialect, text: &str) -> String {
    let text = text.replace('\'', "''");
    match dialect {
        Dialect::Postgres => format!("'{text}'"),
        Dialect::MySql => format!("'{}'", text.replace('\\', "\\\\")),
    }
}

/// The statement that reads a page of `schema.table`.
pub fn page_statement(
    dialect: Dialect,
    schema: &str,
    table: &str,
    filter: &str,
    order: &str,
    skip: u64,
    limit: u64,
) -> (String, String) {
    let from = format!("{}.{}", quote(dialect, schema), quote(dialect, table));
    let filter = filter.trim().trim_end_matches(';');
    let order = order.trim().trim_end_matches(';');
    let mut tail = String::new();
    if !filter.is_empty() {
        tail.push_str(&format!(" WHERE {filter}"));
    }
    let count = format!("SELECT COUNT(*) FROM {from}{tail}");
    if !order.is_empty() {
        tail.push_str(&format!(" ORDER BY {order}"));
    }
    let select = format!("SELECT * FROM {from}{tail} LIMIT {limit} OFFSET {skip}");
    (select, count)
}

// ---- PostgreSQL ---------------------------------------------------------------------------

/// One client per address, reopened when the server closed it.
fn postgres(uri: &str) -> Result<Arc<Mutex<postgres::Client>>> {
    static CLIENTS: OnceLock<Mutex<HashMap<String, Arc<Mutex<postgres::Client>>>>> =
        OnceLock::new();
    let clients = CLIENTS.get_or_init(Default::default);
    if let Some(client) = clients
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(uri)
        .cloned()
        && !client.lock().unwrap_or_else(|e| e.into_inner()).is_closed()
    {
        return Ok(client);
    }
    let mut config = postgres::Config::from_str(uri).context(tr!("database.invalid_uri"))?;
    config.connect_timeout(Duration::from_secs(6));
    if config.get_application_name().is_none() {
        config.application_name("Atelier IDE");
    }
    // As libpq: `require` encrypts without checking the certificate, `verify-…` check it.
    let verify = uri.contains("sslmode=verify");
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(!verify)
        .danger_accept_invalid_hostnames(!verify)
        .build()?;
    let client = config.connect(postgres_native_tls::MakeTlsConnector::new(tls))?;
    let client = Arc::new(Mutex::new(client));
    clients
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(uri.to_string(), client.clone());
    Ok(client)
}

/// Runs statements, the rows of the last one that has rows kept.
fn postgres_query(uri: &str, sql: &str) -> Result<Grid> {
    use postgres::SimpleQueryMessage;
    let client = postgres(uri)?;
    let mut client = client.lock().unwrap_or_else(|e| e.into_inner());
    let messages = client.simple_query(sql)?;
    let mut grid = Grid::default();
    let mut current = Grid::default();
    let mut has_rows = false;
    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                current = Grid {
                    columns: columns.iter().map(|c| c.name().to_string()).collect(),
                    ..Grid::default()
                };
                has_rows = true;
            }
            SimpleQueryMessage::Row(row) => {
                if current.columns.is_empty() {
                    current.columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                }
                current.rows.push(
                    (0..row.len())
                        .map(|i| row.get(i).map(str::to_string))
                        .collect(),
                );
                has_rows = true;
            }
            SimpleQueryMessage::CommandComplete(count) => {
                if has_rows {
                    grid = std::mem::take(&mut current);
                } else {
                    grid = Grid {
                        affected: Some(count),
                        ..Grid::default()
                    };
                }
                has_rows = false;
            }
            _ => {}
        }
    }
    Ok(grid)
}

// ---- MySQL --------------------------------------------------------------------------------

fn mysql(uri: &str) -> Result<mysql::PooledConn> {
    static POOLS: OnceLock<Mutex<HashMap<String, mysql::Pool>>> = OnceLock::new();
    let pools = POOLS.get_or_init(Default::default);
    let pool = pools
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(uri)
        .cloned();
    let pool = match pool {
        Some(pool) => pool,
        None => {
            let opts = mysql::Opts::from_url(uri)
                .map_err(|e| anyhow!("{}: {e}", tr!("database.invalid_uri")))?;
            let opts = mysql::OptsBuilder::from_opts(opts)
                .tcp_connect_timeout(Some(Duration::from_secs(6)))
                .read_timeout(Some(Duration::from_secs(60)));
            let pool = mysql::Pool::new(opts)?;
            pools
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(uri.to_string(), pool.clone());
            pool
        }
    };
    Ok(pool.get_conn()?)
}

fn mysql_value(value: &mysql::Value) -> Option<String> {
    use mysql::Value;
    Some(match value {
        Value::NULL => return None,
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(n) => n.to_string(),
        Value::Double(n) => n.to_string(),
        Value::Date(y, m, d, h, mi, s, us) => {
            if *us > 0 {
                format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
            } else {
                format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            }
        }
        Value::Time(negative, days, h, mi, s, _) => {
            let sign = if *negative { "-" } else { "" };
            format!("{sign}{:02}:{mi:02}:{s:02}", *days * 24 + *h as u32)
        }
    })
}

fn mysql_query(uri: &str, sql: &str) -> Result<Grid> {
    use mysql::prelude::Queryable;
    let mut conn = mysql(uri)?;
    let mut result = conn.query_iter(sql)?;
    let mut grid = Grid::default();
    while let Some(set) = result.iter() {
        let columns: Vec<String> = set
            .columns()
            .as_ref()
            .iter()
            .map(|c| c.name_str().into_owned())
            .collect();
        let affected = set.affected_rows();
        let mut rows = Vec::new();
        for row in set {
            let row = row?;
            rows.push(
                (0..row.len())
                    .map(|i| row.as_ref(i).and_then(mysql_value))
                    .collect(),
            );
        }
        grid = if columns.is_empty() {
            Grid {
                affected: Some(affected),
                ..Grid::default()
            }
        } else {
            Grid {
                columns,
                rows,
                affected: None,
            }
        };
    }
    Ok(grid)
}

// ---- What the viewer asks -----------------------------------------------------------------

pub fn query(dialect: Dialect, uri: &str, sql: &str) -> Result<Grid> {
    match dialect {
        Dialect::Postgres => postgres_query(uri, sql),
        Dialect::MySql => mysql_query(uri, sql),
    }
}

/// The first column of the rows of a statement.
fn names(dialect: Dialect, uri: &str, sql: &str) -> Result<Vec<String>> {
    Ok(query(dialect, uri, sql)?
        .rows
        .into_iter()
        .filter_map(|row| row.into_iter().next().flatten())
        .collect())
}

/// The version of the server.
pub fn test(dialect: Dialect, uri: &str) -> Result<String> {
    let version = names(dialect, uri, "SELECT version()")?
        .into_iter()
        .next()
        .unwrap_or_default();
    // `PostgreSQL 16.2 on x86_64…`: the name and the number.
    Ok(version
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" "))
}

/// The schemas of a PostgreSQL database, the databases of a MySQL server; not the ones of the
/// system.
pub fn schemas(dialect: Dialect, uri: &str) -> Result<Vec<String>> {
    match dialect {
        Dialect::Postgres => names(
            dialect,
            uri,
            "SELECT schema_name FROM information_schema.schemata \
             WHERE schema_name NOT IN ('pg_catalog', 'information_schema') \
             AND schema_name NOT LIKE 'pg\\_toast%' AND schema_name NOT LIKE 'pg\\_temp%' \
             ORDER BY 1",
        ),
        Dialect::MySql => Ok(names(dialect, uri, "SHOW DATABASES")?
            .into_iter()
            .filter(|d| {
                !matches!(
                    d.as_str(),
                    "information_schema" | "mysql" | "performance_schema" | "sys"
                )
            })
            .collect()),
    }
}

/// The tables and views of a schema.
pub fn tables(dialect: Dialect, uri: &str, schema: &str) -> Result<Vec<String>> {
    names(
        dialect,
        uri,
        &format!(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = {} ORDER BY 1",
            literal(dialect, schema)
        ),
    )
}

/// A page of a table: the rows, and how many match.
#[allow(clippy::too_many_arguments)]
pub fn page(
    dialect: Dialect,
    uri: &str,
    schema: &str,
    table: &str,
    filter: &str,
    order: &str,
    skip: u64,
    limit: u64,
) -> Result<Page> {
    let (select, count) = page_statement(dialect, schema, table, filter, order, skip, limit);
    let grid = query(dialect, uri, &select)?;
    let total = names(dialect, uri, &count)?
        .first()
        .and_then(|n| n.parse().ok())
        .unwrap_or(grid.rows.len() as u64);
    Ok(Page { grid, total })
}

// ---- Structure and changes ----------------------------------------------------------------

/// A column of a table.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// Part of the primary key.
    pub key: bool,
}

/// The columns of a table, in order, with the ones of its primary key marked.
pub fn columns(dialect: Dialect, uri: &str, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
    let (schema, table) = (literal(dialect, schema), literal(dialect, table));
    let grid = query(
        dialect,
        uri,
        &format!(
            "SELECT c.column_name, c.data_type, c.is_nullable, c.column_default, \
             CASE WHEN EXISTS (SELECT 1 FROM information_schema.table_constraints tc \
               JOIN information_schema.key_column_usage k \
                 ON tc.constraint_name = k.constraint_name \
                AND tc.table_schema = k.table_schema AND tc.table_name = k.table_name \
               WHERE tc.constraint_type = 'PRIMARY KEY' AND k.table_schema = c.table_schema \
                 AND k.table_name = c.table_name AND k.column_name = c.column_name) \
             THEN 1 ELSE 0 END \
             FROM information_schema.columns c \
             WHERE c.table_schema = {schema} AND c.table_name = {table} \
             ORDER BY c.ordinal_position"
        ),
    )?;
    Ok(grid
        .rows
        .into_iter()
        .map(|row| {
            let text = |i: usize| row.get(i).cloned().flatten();
            ColumnInfo {
                name: text(0).unwrap_or_default(),
                data_type: text(1).unwrap_or_default(),
                nullable: text(2).is_some_and(|n| n.eq_ignore_ascii_case("YES")),
                default: text(3),
                key: text(4).is_some_and(|k| k == "1"),
            }
        })
        .collect())
}

/// A value of a statement: a literal, or `NULL`.
fn value(dialect: Dialect, value: &Option<String>) -> String {
    match value {
        Some(text) => literal(dialect, text),
        None => "NULL".into(),
    }
}

/// `"schema"."table"`.
pub fn qualified(dialect: Dialect, schema: &str, table: &str) -> String {
    format!("{}.{}", quote(dialect, schema), quote(dialect, table))
}

/// The condition that finds a row by its primary key: None for a table without one.
fn by_key(
    dialect: Dialect,
    columns: &[ColumnInfo],
    names: &[String],
    row: &[Option<String>],
) -> Option<String> {
    let keys: Vec<&ColumnInfo> = columns.iter().filter(|c| c.key).collect();
    if keys.is_empty() {
        return None;
    }
    let parts: Option<Vec<String>> = keys
        .iter()
        .map(|key| {
            let index = names.iter().position(|n| *n == key.name)?;
            Some(match &row[index] {
                Some(v) => format!("{} = {}", quote(dialect, &key.name), literal(dialect, v)),
                None => format!("{} IS NULL", quote(dialect, &key.name)),
            })
        })
        .collect();
    Some(parts?.join(" AND "))
}

/// A row of a table, to change it.
pub struct RowRef<'a> {
    pub dialect: Dialect,
    pub schema: &'a str,
    pub table: &'a str,
    pub columns: &'a [ColumnInfo],
    /// The names of the columns of the grid, in its order.
    pub names: &'a [String],
    pub row: &'a [Option<String>],
}

impl RowRef<'_> {
    /// The statement that sets one value of the row, found by its primary key.
    pub fn update(&self, column: &str, new: &Option<String>) -> Option<String> {
        let condition = by_key(self.dialect, self.columns, self.names, self.row)?;
        Some(format!(
            "UPDATE {} SET {} = {} WHERE {condition}",
            qualified(self.dialect, self.schema, self.table),
            quote(self.dialect, column),
            value(self.dialect, new)
        ))
    }

    /// The statement that deletes the row, found by its primary key.
    pub fn delete(&self) -> Option<String> {
        let condition = by_key(self.dialect, self.columns, self.names, self.row)?;
        Some(format!(
            "DELETE FROM {} WHERE {condition}",
            qualified(self.dialect, self.schema, self.table)
        ))
    }
}

/// The statement that adds a row; the columns left out take their default.
pub fn insert_statement(
    dialect: Dialect,
    schema: &str,
    table: &str,
    values: &[(String, Option<String>)],
) -> String {
    let target = qualified(dialect, schema, table);
    if values.is_empty() {
        return match dialect {
            Dialect::Postgres => format!("INSERT INTO {target} DEFAULT VALUES"),
            Dialect::MySql => format!("INSERT INTO {target} () VALUES ()"),
        };
    }
    let names: Vec<String> = values.iter().map(|(n, _)| quote(dialect, n)).collect();
    let literals: Vec<String> = values.iter().map(|(_, v)| value(dialect, v)).collect();
    format!(
        "INSERT INTO {target} ({}) VALUES ({})",
        names.join(", "),
        literals.join(", ")
    )
}

/// A column of a table to create.
#[derive(Clone, Debug, PartialEq)]
pub struct NewColumn {
    pub name: String,
    pub data_type: String,
    pub key: bool,
    pub not_null: bool,
    /// An expression, as written: `0`, `'none'`, `now()`.
    pub default: String,
}

/// The types offered for a new column.
pub fn types(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => &[
            "serial",
            "bigserial",
            "integer",
            "bigint",
            "smallint",
            "numeric(10,2)",
            "real",
            "double precision",
            "boolean",
            "text",
            "varchar(255)",
            "char(1)",
            "date",
            "timestamp",
            "timestamptz",
            "time",
            "uuid",
            "json",
            "jsonb",
            "bytea",
        ],
        Dialect::MySql => &[
            "INT AUTO_INCREMENT",
            "INT",
            "BIGINT",
            "SMALLINT",
            "TINYINT(1)",
            "DECIMAL(10,2)",
            "FLOAT",
            "DOUBLE",
            "BOOLEAN",
            "VARCHAR(255)",
            "TEXT",
            "LONGTEXT",
            "CHAR(1)",
            "DATE",
            "DATETIME",
            "TIMESTAMP",
            "TIME",
            "JSON",
            "BLOB",
        ],
    }
}

/// The column a new table starts with: an identifier that counts by itself.
pub fn identity_column(dialect: Dialect) -> NewColumn {
    NewColumn {
        name: "id".into(),
        data_type: types(dialect)[0].into(),
        key: true,
        not_null: true,
        default: String::new(),
    }
}

/// The statement that creates a table.
pub fn create_table_statement(
    dialect: Dialect,
    schema: &str,
    table: &str,
    columns: &[NewColumn],
) -> String {
    let named: Vec<&NewColumn> = columns
        .iter()
        .filter(|c| !c.name.trim().is_empty())
        .collect();
    let mut lines: Vec<String> = named
        .iter()
        .map(|c| {
            let mut line = format!("  {} {}", quote(dialect, c.name.trim()), c.data_type.trim());
            if c.not_null || c.key {
                line.push_str(" NOT NULL");
            }
            if !c.default.trim().is_empty() {
                line.push_str(&format!(" DEFAULT {}", c.default.trim()));
            }
            line
        })
        .collect();
    let keys: Vec<String> = named
        .iter()
        .filter(|c| c.key)
        .map(|c| quote(dialect, c.name.trim()))
        .collect();
    if !keys.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", keys.join(", ")));
    }
    format!(
        "CREATE TABLE {} (\n{}\n)",
        qualified(dialect, schema, table),
        lines.join(",\n")
    )
}

/// The statement that creates a schema (PostgreSQL) or a database (MySQL).
pub fn create_schema_statement(dialect: Dialect, name: &str) -> String {
    match dialect {
        Dialect::Postgres => format!("CREATE SCHEMA {}", quote(dialect, name)),
        Dialect::MySql => format!("CREATE DATABASE {}", quote(dialect, name)),
    }
}

pub fn drop_schema_statement(dialect: Dialect, name: &str) -> String {
    match dialect {
        Dialect::Postgres => format!("DROP SCHEMA {} CASCADE", quote(dialect, name)),
        Dialect::MySql => format!("DROP DATABASE {}", quote(dialect, name)),
    }
}

/// Runs statements in a schema: the names written without one are the schema's.
pub fn query_in(dialect: Dialect, uri: &str, schema: Option<&str>, sql: &str) -> Result<Grid> {
    let Some(schema) = schema.filter(|s| !s.is_empty()) else {
        return query(dialect, uri, sql);
    };
    let enter = match dialect {
        Dialect::Postgres => format!("SET search_path TO {}, public", quote(dialect, schema)),
        Dialect::MySql => format!("USE {}", quote(dialect, schema)),
    };
    // One text of statements: they run on the same connection.
    query(dialect, uri, &format!("{enter};\n{sql}"))
}
