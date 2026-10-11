//! The MCP server: JSON-RPC 2.0 over HTTP (the "streamable HTTP" transport of the Model Context
//! Protocol), on the loopback interface only, behind a bearer token.
//!
//! Every connection is a session of the project ([`super::sessions`]): `initialize` opens it and
//! hands its id to the client (`Mcp-Session-Id`), every tool call is recorded in it, and the tools
//! that keep the context of the session are answered here, from the store.
//!
//! Requests are answered by a thread per connection. A tool that needs the editor is handed to
//! the interface thread through a channel ([`Call`]) and answered at its next frame; a tool that
//! does not (the project search) is run by the connection's thread itself, so the window never
//! waits for it.

use super::sessions::Store;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// The versions of the protocol this server speaks, newest first.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_HEADERS: usize = 64 * 1024;
const MAX_BODY: usize = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a tool may take before the AI is told it did not answer.
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// A tool call for the interface thread.
pub struct Call {
    pub tool: String,
    pub args: Value,
    pub reply: mpsc::Sender<Result<String, String>>,
}

pub struct Server {
    pub port: u16,
    pub calls: mpsc::Receiver<Call>,
    /// The project, kept up to date by the plugin, for the tools that run off the interface.
    pub root: Arc<Mutex<PathBuf>>,
    /// The sessions of the project.
    pub store: Arc<Mutex<Store>>,
}

/// Tools that do not touch the editor: they run on the thread of the connection.
fn background_tool(name: &str, root: &Path, args: &Value) -> Option<Result<String, String>> {
    if name == "search_in_project" {
        return Some(super::tools::search(root, args));
    }
    super::commits::tool(name, args, root)
        .or_else(|| super::rework::tool(name, args, root))
        .or_else(|| super::tools::graph(name, args))
        .or_else(|| crate::plugins::builtins::translations::mcp_tool(name, args, root))
}

/// Starts listening on `port` (0 for any), answering with `token` as the secret. `wake` is called
/// when a tool call is waiting for the interface thread.
pub fn start(
    port: u16,
    token: String,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> std::io::Result<Server> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .or_else(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))?;
    let port = listener.local_addr()?.port();
    let (calls_tx, calls) = mpsc::channel();
    let root = Arc::new(Mutex::new(PathBuf::new()));
    let store = Arc::new(Mutex::new(Store::default()));
    let (shared_root, shared_store) = (root.clone(), store.clone());
    std::thread::Builder::new()
        .name("mcp-server".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let (token, calls_tx, wake, root, store) = (
                    token.clone(),
                    calls_tx.clone(),
                    wake.clone(),
                    shared_root.clone(),
                    shared_store.clone(),
                );
                let _ = std::thread::Builder::new()
                    .name("mcp-connection".into())
                    .spawn(move || {
                        let _ = serve(stream, port, &token, &calls_tx, &*wake, &root, &store);
                    });
            }
        })?;
    Ok(Server {
        port,
        calls,
        root,
        store,
    })
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .by_ref()
        .take(MAX_HEADERS as u64)
        .read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let mut headers = Vec::new();
    let mut size = line.len();
    loop {
        let mut header = String::new();
        let n = reader
            .by_ref()
            .take(MAX_HEADERS as u64)
            .read_line(&mut header)?;
        size += n;
        if n == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if size > MAX_HEADERS {
            return Err(std::io::Error::other(tr!("mcp.headers_too_long")));
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(std::io::Error::other(tr!("mcp.body_too_long")));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn respond(mut stream: &TcpStream, status: &str, body: &str, extra: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

/// Whether the request comes from a page of the browser rather than a program: a page of any
/// site could otherwise reach a server of the loopback interface.
fn from_browser(request: &Request, port: u16) -> bool {
    let local = |value: &str| {
        let host = value
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let host = host.split('/').next().unwrap_or("");
        host == format!("127.0.0.1:{port}")
            || host == format!("localhost:{port}")
            || host == "127.0.0.1"
            || host == "localhost"
    };
    let bad_host = request.header("host").is_some_and(|h| !local(h));
    let bad_origin = request.header("origin").is_some_and(|o| !local(o));
    bad_host || bad_origin
}

fn serve(
    stream: TcpStream,
    port: u16,
    token: &str,
    calls: &mpsc::Sender<Call>,
    wake: &(dyn Fn() + Send + Sync),
    root: &Arc<Mutex<PathBuf>>,
    store: &Mutex<Store>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let request = read_request(&stream)?;
    if from_browser(&request, port) {
        return respond(&stream, "403 Forbidden", r#"{"error":"forbidden"}"#, "");
    }
    let authorized = request
        .header("authorization")
        .is_some_and(|a| a.strip_prefix("Bearer ").is_some_and(|t| t.trim() == token));
    if !authorized {
        return respond(
            &stream,
            "401 Unauthorized",
            r#"{"error":"missing or wrong bearer token"}"#,
            "WWW-Authenticate: Bearer\r\n",
        );
    }
    if request.path.split('?').next() != Some("/mcp") {
        return respond(&stream, "404 Not Found", r#"{"error":"not found"}"#, "");
    }
    match request.method.as_str() {
        "POST" => {}
        // The client closes its session. There is no stream of server messages (it is optional).
        "DELETE" => {
            if let Some(id) = request.header("mcp-session-id") {
                store.lock().unwrap_or_else(|e| e.into_inner()).end(id);
            }
            return respond(&stream, "200 OK", "{}", "");
        }
        _ => {
            return respond(
                &stream,
                "405 Method Not Allowed",
                r#"{"error":"POST only"}"#,
                "Allow: POST\r\n",
            );
        }
    }
    let message: Value = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(e) => {
            let body = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": trf!("mcp.invalid_json", e = e)}});
            return respond(&stream, "400 Bad Request", &body.to_string(), "");
        }
    };
    let root = root.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut session = request.header("mcp-session-id").map(str::to_string);
    let mut opened = None;
    let mut one = |m: &Value| {
        let (answer, new_session) = handle(m, calls, wake, &root, store, session.as_deref());
        if let Some(id) = new_session {
            session = Some(id.clone());
            opened = Some(id);
        }
        answer
    };
    let answer = match &message {
        Value::Array(batch) => {
            let answers: Vec<Value> = batch.iter().filter_map(&mut one).collect();
            (!answers.is_empty()).then(|| Value::Array(answers))
        }
        single => one(single),
    };
    match answer {
        Some(answer) => {
            let header = opened
                .map(|id| format!("Mcp-Session-Id: {id}\r\n"))
                .unwrap_or_default();
            respond(&stream, "200 OK", &answer.to_string(), &header)
        }
        // Only notifications: nothing to say.
        None => respond(&stream, "202 Accepted", "", ""),
    }
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// What a call is recorded as in its session: the part of its arguments that says what it was.
fn summarize(tool: &str, args: &Value) -> String {
    match tool {
        "open_file" => format!(
            "{}:{}",
            args["path"].as_str().unwrap_or("?"),
            args["line"].as_u64().unwrap_or(1)
        ),
        "search_in_project" | "session_note" => args["query"]
            .as_str()
            .or(args["text"].as_str())
            .unwrap_or("")
            .to_string(),
        _ => args.to_string(),
    }
}

/// The tools of the context of a session: answered from the store, on this thread.
fn session_tool(
    name: &str,
    args: &Value,
    store: &Mutex<Store>,
    session: &str,
) -> Option<Result<String, String>> {
    let mut store = store.lock().unwrap_or_else(|e| e.into_inner());
    Some(match name {
        "session_note" => {
            let (kind, text) = (
                args["kind"].as_str().unwrap_or("progress"),
                args["text"].as_str().unwrap_or(""),
            );
            store
                .note(session, kind, text)
                .map(|()| trf!("mcp.noted_session", kind = kind, session = session))
        }
        "get_session_context" => {
            let wanted = args["id"].as_str().unwrap_or(session).to_string();
            match store.markdown(&wanted) {
                Some(text) => {
                    // An AI that asks for another session is taking it up: this one says so.
                    if wanted != session {
                        let _ = store.note(
                            session,
                            "progress",
                            &trf!("mcp.taking_up_session", wanted = wanted),
                        );
                    }
                    Ok(text)
                }
                None => Err(trf!("mcp.unknown_session", wanted = wanted)),
            }
        }
        "list_sessions" => {
            let limit = args["limit"].as_u64().unwrap_or(20).min(100) as usize;
            let at = super::sessions::now();
            let list: Vec<Value> = store
                .sessions
                .iter()
                .take(limit)
                .map(|s| {
                    json!({
                        "id": s.id,
                        "title": s.display_title(),
                        "client": s.client,
                        "started": super::sessions::clock(s.started),
                        "active": s.active(at),
                        "calls": s.events.len(),
                        "notes": s.notes.len(),
                    })
                })
                .collect();
            serde_json::to_string_pretty(&json!({"current": session, "sessions": list}))
                .map_err(|e| e.to_string())
        }
        _ => return None,
    })
}

/// One JSON-RPC message, and the session it opened if it was `initialize`. The answer is None
/// for a notification, which is not answered.
fn handle(
    message: &Value,
    calls: &mpsc::Sender<Call>,
    wake: &(dyn Fn() + Send + Sync),
    root: &Path,
    store: &Mutex<Store>,
    session: Option<&str>,
) -> (Option<Value>, Option<String>) {
    let Some(id) = message.get("id").cloned() else {
        return (None, None);
    };
    let method = message["method"].as_str().unwrap_or("");
    let params = &message["params"];
    let mut opened = None;
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOLS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&PROTOCOLS[0]);
            let client = &params["clientInfo"];
            opened = Some(store.lock().unwrap_or_else(|e| e.into_inner()).start(
                client["name"].as_str().unwrap_or(""),
                client["version"].as_str().unwrap_or(""),
            ));
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}, "prompts": {"listChanged": false}},
                "serverInfo": {"name": "atelier", "title": "Atelier IDE", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Outils de l'IDE Atelier : fichiers ouverts, éditeur actif, problèmes du code, recherche dans le projet, ouverture d'un fichier à une ligne. \
                    Atelier garde le contexte de cette session dans le projet : au début, note l'objectif avec session_note (kind goal), \
                    puis les décisions (decision), les tâches restantes (todo) et l'avancement (progress). Une autre IA pourra reprendre la session à partir de ces notes (get_session_context). \
                    If the project's context graph is on, read graph_overview then ask graph_query / graph_node before opening many files: it costs far fewer tokens."
            })
        }
        "ping" => json!({}),
        "prompts/list" => {
            json!({"prompts": super::commits::prompts().into_iter().chain(super::rework::prompts()).collect::<Vec<_>>()})
        }
        "prompts/get" => {
            let name = params["name"].as_str().unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match super::commits::prompt(name, &arguments)
                .or_else(|| super::rework::prompt(name, &arguments))
            {
                Some(text) => {
                    json!({"description": format!("Atelier : {name}"), "messages": [{"role": "user", "content": {"type": "text", "text": text}}]})
                }
                None => {
                    return (
                        Some(error(
                            &id,
                            -32602,
                            &trf!("mcp.unknown_command", name = name),
                        )),
                        None,
                    );
                }
            }
        }
        "tools/list" => json!({"tools": super::tools::specs()}),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("").to_string();
            if !super::tools::specs()
                .iter()
                .any(|t| t["name"] == name.as_str())
            {
                return (
                    Some(error(&id, -32602, &trf!("mcp.unknown_tool", name = name))),
                    None,
                );
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let current = store
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .current(session);
            let outcome = match session_tool(&name, &args, store, &current)
                .or_else(|| background_tool(&name, root, &args))
            {
                Some(outcome) => outcome,
                None => {
                    let (reply, answer) = mpsc::channel();
                    if calls
                        .send(Call {
                            tool: name.clone(),
                            args: args.clone(),
                            reply,
                        })
                        .is_err()
                    {
                        return (
                            Some(error(&id, -32603, tr!("mcp.ide_no_longer_listens"))),
                            None,
                        );
                    }
                    wake();
                    answer
                        .recv_timeout(TOOL_TIMEOUT)
                        .unwrap_or_else(|_| Err(tr!("mcp.ide_did_not_answer_time").into()))
                }
            };
            store.lock().unwrap_or_else(|e| e.into_inner()).record(
                &current,
                &name,
                &summarize(&name, &args),
                outcome.is_ok(),
            );
            let (text, is_error) = match outcome {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": is_error})
        }
        _ => {
            return (
                Some(error(
                    &id,
                    -32601,
                    &trf!("mcp.unknown_method", method = method),
                )),
                None,
            );
        }
    };
    (
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
        opened,
    )
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/mcp/server/tests.rs"]
mod tests;
