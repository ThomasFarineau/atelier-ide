//! The MongoDB driver of the database viewer: what the viewer asks of a server, done with the
//! official client, in its blocking form. Every call runs on a thread of its own (see
//! `mod.rs`), never on the one of the interface.
//!
//! Documents travel as relaxed Extended JSON (`{"_id": {"$oid": "…"}, "n": 1}`): what the
//! viewer shows and edits, and what is turned back into BSON to be written.

use anyhow::{Context, Result, anyhow, bail};
use mongodb::{
    bson::{Bson, Document, doc},
    options::ClientOptions,
    sync::Client,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// The clients made, by address.
fn clients() -> &'static Mutex<HashMap<String, Client>> {
    static CLIENTS: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    CLIENTS.get_or_init(Default::default)
}

/// One client per address: a client keeps its connections, making one is slow.
fn client(uri: &str) -> Result<Client> {
    let clients = clients();
    if let Some(client) = clients.lock().unwrap_or_else(|e| e.into_inner()).get(uri) {
        return Ok(client.clone());
    }
    let mut options = ClientOptions::parse(uri)
        .run()
        .with_context(|| tr!("database.invalid_uri").to_string())?;
    // An unreachable server says so in a few seconds rather than thirty.
    options.server_selection_timeout = Some(Duration::from_secs(6));
    options.connect_timeout = Some(Duration::from_secs(6));
    if options.app_name.is_none() {
        options.app_name = Some("Atelier IDE".into());
    }
    let client = Client::with_options(options)?;
    clients
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(uri.to_string(), client.clone());
    Ok(client)
}

/// Forgets the client of an address (its connection was edited or removed).
pub fn forget(uri: &str) {
    clients()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(uri);
}

/// The database named in the address (`mongodb://host/name`), if any.
fn default_database(uri: &str) -> Option<String> {
    let rest = uri.split_once("://")?.1;
    let path = rest.split_once('/')?.1;
    let name = path.split(['?', '/']).next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Checks that the server answers: its version.
pub fn test(uri: &str) -> Result<String> {
    let client = client(uri)?;
    let info = client
        .database("admin")
        .run_command(doc! {"buildInfo": 1})
        .run()?;
    Ok(info.get_str("version").unwrap_or("?").to_string())
}

/// The databases of the server; the one of the address when the user may not list them.
pub fn databases(uri: &str) -> Result<Vec<String>> {
    let client = client(uri)?;
    match client.list_database_names().run() {
        Ok(mut names) => {
            names.sort();
            Ok(names)
        }
        Err(error) => match default_database(uri) {
            Some(name) => Ok(vec![name]),
            None => Err(error.into()),
        },
    }
}

pub fn collections(uri: &str, database: &str) -> Result<Vec<String>> {
    let client = client(uri)?;
    let mut names = client.database(database).list_collection_names().run()?;
    names.sort();
    Ok(names)
}

/// A query of the viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub database: String,
    pub collection: String,
    /// JSON (or the shell's way of writing it: `{age: {$gt: 30}}`); empty for every document.
    pub filter: String,
    pub sort: String,
    pub skip: u64,
    pub limit: i64,
}

/// A page of documents and how many match.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub documents: Vec<Value>,
    pub total: u64,
}

/// JSON written as the MongoDB shell writes it, made strict: keys without quotes get them,
/// strings between single quotes get double ones.
pub fn strict_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    // What came before, outside strings: a key follows `{` or `,`.
    let mut expects_key = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                out.push(c);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    if chars[i] == '"' {
                        break;
                    }
                    i += 1;
                }
                i += 1;
                expects_key = false;
            }
            '\'' => {
                out.push('"');
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    if chars[i] == '"' {
                        out.push('\\');
                    }
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        out.push(chars[i]);
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                out.push('"');
                i += 1;
                expects_key = false;
            }
            '{' | ',' => {
                out.push(c);
                i += 1;
                expects_key = true;
            }
            c if expects_key && (c.is_alphabetic() || c == '_' || c == '$') => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '$' | '.'))
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                out.push('"');
                out.push_str(&word);
                out.push('"');
                expects_key = false;
            }
            c => {
                if !c.is_whitespace() {
                    expects_key = false;
                }
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// A document from JSON, the shell's way of writing it accepted.
pub fn document(text: &str) -> Result<Document> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Document::new());
    }
    let value: Value = serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&strict_json(text)))
        .map_err(|e| anyhow!(trf!("database.invalid_json", e = e)))?;
    match Bson::try_from(value).map_err(|e| anyhow!(e.to_string()))? {
        Bson::Document(document) => Ok(document),
        _ => bail!(tr!("database.not_an_object")),
    }
}

/// A document as the viewer shows it.
pub fn to_json(document: Document) -> Value {
    Bson::Document(document).into_relaxed_extjson()
}

pub fn find(uri: &str, query: &Query) -> Result<Page> {
    let client = client(uri)?;
    let collection = client
        .database(&query.database)
        .collection::<Document>(&query.collection);
    let filter = document(&query.filter)?;
    let sort = document(&query.sort)?;
    let mut find = collection
        .find(filter.clone())
        .skip(query.skip)
        .limit(query.limit.max(1));
    if !sort.is_empty() {
        find = find.sort(sort);
    }
    let documents = find
        .run()?
        .map(|d| d.map(to_json))
        .collect::<std::result::Result<Vec<Value>, _>>()?;
    let total = if filter.is_empty() {
        collection.estimated_document_count().run()?
    } else {
        collection.count_documents(filter).run()?
    };
    Ok(Page { documents, total })
}

/// The `_id` of a shown document, as a filter that finds it.
fn by_id(shown: &Value) -> Result<Document> {
    let id = shown
        .get("_id")
        .cloned()
        .ok_or_else(|| anyhow!(tr!("database.no_id")))?;
    let id = Bson::try_from(id).map_err(|e| anyhow!(e.to_string()))?;
    Ok(doc! {"_id": id})
}

/// Replaces the document shown as `shown` by the one written in `text`.
pub fn replace(
    uri: &str,
    database: &str,
    collection: &str,
    shown: &Value,
    text: &str,
) -> Result<()> {
    let client = client(uri)?;
    let replacement = document(text)?;
    let result = client
        .database(database)
        .collection::<Document>(collection)
        .replace_one(by_id(shown)?, replacement)
        .run()?;
    if result.matched_count == 0 {
        bail!(tr!("database.document_gone"));
    }
    Ok(())
}

pub fn insert(uri: &str, database: &str, collection: &str, text: &str) -> Result<()> {
    let client = client(uri)?;
    client
        .database(database)
        .collection::<Document>(collection)
        .insert_one(document(text)?)
        .run()?;
    Ok(())
}

pub fn delete(uri: &str, database: &str, collection: &str, shown: &Value) -> Result<()> {
    let client = client(uri)?;
    client
        .database(database)
        .collection::<Document>(collection)
        .delete_one(by_id(shown)?)
        .run()?;
    Ok(())
}

/// Creates a collection; a database is created with its first collection.
pub fn create_collection(uri: &str, database: &str, collection: &str) -> Result<()> {
    client(uri)?
        .database(database)
        .create_collection(collection)
        .run()?;
    Ok(())
}

pub fn drop_collection(uri: &str, database: &str, collection: &str) -> Result<()> {
    client(uri)?
        .database(database)
        .collection::<Document>(collection)
        .drop()
        .run()?;
    Ok(())
}

pub fn drop_database(uri: &str, database: &str) -> Result<()> {
    client(uri)?.database(database).drop().run()?;
    Ok(())
}
