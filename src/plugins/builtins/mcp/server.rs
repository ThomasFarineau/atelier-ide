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
    path::PathBuf,
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
fn background_tool(name: &str, root: &PathBuf, args: &Value) -> Option<Result<String, String>> {
    if name == "search_in_project" {
        return Some(super::tools::search(root, args));
    }
    super::commits::tool(name, args, root).or_else(|| super::tools::graph(name, args))
}

/// Starts listening on `port` (0 for any), answering with `token` as the secret. `wake` is called
/// when a tool call is waiting for the interface thread.
pub fn start(port: u16, token: String, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Server> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).or_else(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))?;
    let port = listener.local_addr()?.port();
    let (calls_tx, calls) = mpsc::channel();
    let root = Arc::new(Mutex::new(PathBuf::new()));
    let store = Arc::new(Mutex::new(Store::default()));
    let (shared_root, shared_store) = (root.clone(), store.clone());
    std::thread::Builder::new().name("mcp-server".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let (token, calls_tx, wake, root, store) =
                (token.clone(), calls_tx.clone(), wake.clone(), shared_root.clone(), shared_store.clone());
            let _ = std::thread::Builder::new().name("mcp-connection".into()).spawn(move || {
                let _ = serve(stream, port, &token, &calls_tx, &*wake, &root, &store);
            });
        }
    })?;
    Ok(Server { port, calls, root, store })
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.by_ref().take(MAX_HEADERS as u64).read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut headers = Vec::new();
    let mut size = line.len();
    loop {
        let mut header = String::new();
        let n = reader.by_ref().take(MAX_HEADERS as u64).read_line(&mut header)?;
        size += n;
        if n == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if size > MAX_HEADERS {
            return Err(std::io::Error::other("en-têtes trop longs"));
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
        return Err(std::io::Error::other("corps trop long"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, headers, body })
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
        let host = value.trim_start_matches("http://").trim_start_matches("https://");
        let host = host.split('/').next().unwrap_or("");
        host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}") || host == "127.0.0.1" || host == "localhost"
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
    let authorized = request.header("authorization").is_some_and(|a| a.strip_prefix("Bearer ").is_some_and(|t| t.trim() == token));
    if !authorized {
        return respond(&stream, "401 Unauthorized", r#"{"error":"missing or wrong bearer token"}"#, "WWW-Authenticate: Bearer\r\n");
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
        _ => return respond(&stream, "405 Method Not Allowed", r#"{"error":"POST only"}"#, "Allow: POST\r\n"),
    }
    let message: Value = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(e) => {
            let body = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("JSON invalide : {e}")}});
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
            let header = opened.map(|id| format!("Mcp-Session-Id: {id}\r\n")).unwrap_or_default();
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
        "open_file" => format!("{}:{}", args["path"].as_str().unwrap_or("?"), args["line"].as_u64().unwrap_or(1)),
        "search_in_project" | "session_note" => args["query"].as_str().or(args["text"].as_str()).unwrap_or("").to_string(),
        _ => args.to_string(),
    }
}

/// The tools of the context of a session: answered from the store, on this thread.
fn session_tool(name: &str, args: &Value, store: &Mutex<Store>, session: &str) -> Option<Result<String, String>> {
    let mut store = store.lock().unwrap_or_else(|e| e.into_inner());
    Some(match name {
        "session_note" => {
            let (kind, text) = (args["kind"].as_str().unwrap_or("progress"), args["text"].as_str().unwrap_or(""));
            store.note(session, kind, text).map(|()| format!("Noté ({kind}) dans la session {session}"))
        }
        "get_session_context" => {
            let wanted = args["id"].as_str().unwrap_or(session).to_string();
            match store.markdown(&wanted) {
                Some(text) => {
                    // An AI that asks for another session is taking it up: this one says so.
                    if wanted != session {
                        let _ = store.note(session, "progress", &format!("Reprise de la session {wanted}"));
                    }
                    Ok(text)
                }
                None => Err(format!("Session inconnue : {wanted}")),
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
            serde_json::to_string_pretty(&json!({"current": session, "sessions": list})).map_err(|e| e.to_string())
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
    root: &PathBuf,
    store: &Mutex<Store>,
    session: Option<&str>,
) -> (Option<Value>, Option<String>) {
    let Some(id) = message.get("id").cloned() else { return (None, None) };
    let method = message["method"].as_str().unwrap_or("");
    let params = &message["params"];
    let mut opened = None;
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOLS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOLS[0]);
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
        "prompts/list" => json!({"prompts": super::commits::prompts()}),
        "prompts/get" => {
            let name = params["name"].as_str().unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match super::commits::prompt(name, &arguments) {
                Some(text) => json!({"description": format!("Atelier : {name}"), "messages": [{"role": "user", "content": {"type": "text", "text": text}}]}),
                None => return (Some(error(&id, -32602, &format!("commande inconnue : {name}"))), None),
            }
        }
        "tools/list" => json!({"tools": super::tools::specs()}),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("").to_string();
            if !super::tools::specs().iter().any(|t| t["name"] == name.as_str()) {
                return (Some(error(&id, -32602, &format!("outil inconnu : {name}"))), None);
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let current = store.lock().unwrap_or_else(|e| e.into_inner()).current(session);
            let outcome = match session_tool(&name, &args, store, &current).or_else(|| background_tool(&name, root, &args)) {
                Some(outcome) => outcome,
                None => {
                    let (reply, answer) = mpsc::channel();
                    if calls.send(Call { tool: name.clone(), args: args.clone(), reply }).is_err() {
                        return (Some(error(&id, -32603, "l'IDE n'écoute plus")), None);
                    }
                    wake();
                    answer.recv_timeout(TOOL_TIMEOUT).unwrap_or_else(|_| Err("l'IDE n'a pas répondu à temps".into()))
                }
            };
            store.lock().unwrap_or_else(|e| e.into_inner()).record(&current, &name, &summarize(&name, &args), outcome.is_ok());
            let (text, is_error) = match outcome {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": is_error})
        }
        _ => return (Some(error(&id, -32601, &format!("méthode inconnue : {method}"))), None),
    };
    (Some(json!({"jsonrpc": "2.0", "id": id, "result": result})), opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One request to the server, as an MCP client would send it: the status and the body.
    fn post(port: u16, token: Option<&str>, extra: &str, body: &str) -> (u16, String) {
        let (status, _, body) = post_with_headers(port, token, extra, body);
        (status, body)
    }

    /// The same, with the headers of the answer.
    fn post_with_headers(port: u16, token: Option<&str>, extra: &str, body: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        write!(
            stream,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        let status = reply.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let (head, body) = reply.split_once("\r\n\r\n").map(|(h, b)| (h.to_string(), b.to_string())).unwrap_or_default();
        (status, head, body)
    }

    fn running() -> (Server, u16) {
        let server = start(0, "secret".into(), Arc::new(|| {})).unwrap();
        let port = server.port;
        (server, port)
    }

    #[test]
    fn a_client_initializes_lists_the_tools_and_calls_one() {
        let (server, port) = running();
        let (status, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#);
        assert_eq!(status, 200);
        let answer: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(answer["result"]["protocolVersion"], "2025-03-26", "the version the client asked for");
        assert_eq!(answer["result"]["serverInfo"]["name"], "atelier");
        let (status, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert_eq!((status, body.as_str()), (202, ""), "a notification is not answered");
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools: Value = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"get_problems") && names.contains(&"open_file") && names.contains(&"search_in_project"), "{names:?}");
        // A tool of the editor goes through the channel and is answered by the interface thread.
        let answering = std::thread::spawn(move || {
            let call = server.calls.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(call.tool, "get_open_files");
            call.reply.send(Ok("[]".into())).unwrap();
        });
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_open_files","arguments":{}}}"#);
        answering.join().unwrap();
        let answer: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(answer["result"]["content"][0]["text"], "[]");
        assert_eq!(answer["result"]["isError"], false);
    }

    #[test]
    fn unknown_methods_and_tools_get_json_rpc_errors() {
        let (_server, port) = running();
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":7,"method":"resources/list"}"#);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"]["code"], -32601);
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"format_disk"}}"#);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"]["code"], -32602);
        let (status, _) = post(port, Some("secret"), "", "pas du json");
        assert_eq!(status, 400);
    }

    #[test]
    fn without_the_token_or_from_a_web_page_nothing_is_answered() {
        let (_server, port) = running();
        let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        assert_eq!(post(port, None, "", ping).0, 401);
        assert_eq!(post(port, Some("wrong"), "", ping).0, 401);
        assert_eq!(post(port, Some("secret"), "Origin: https://evil.example\r\n", ping).0, 403, "a page of the browser");
        assert_eq!(post(port, Some("secret"), "", ping).0, 200);
    }

    #[test]
    fn a_connection_is_a_session_that_records_its_calls_and_keeps_its_notes() {
        let (server, port) = running();
        let dir = std::env::temp_dir().join(format!("atelier-mcp-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        server.store.lock().unwrap().set_root(&dir);
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"claude-code","version":"2.1"}}}"#;
        let (_, head, _) = post_with_headers(port, Some("secret"), "", init);
        let id = head.lines().find_map(|l| l.strip_prefix("Mcp-Session-Id: ")).expect("the client is given its session").trim().to_string();
        let with_session = format!("Mcp-Session-Id: {id}\r\n");
        let note = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"session_note","arguments":{"kind":"goal","text":"Réparer le build"}}}"#;
        let (_, body) = post(port, Some("secret"), &with_session, note);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["result"]["isError"], false, "{body}");
        let search = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_in_project","arguments":{"query":"fn main"}}}"#;
        post(port, Some("secret"), &with_session, search);
        let context = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_session_context","arguments":{}}}"#;
        let (_, body) = post(port, Some("secret"), &with_session, context);
        let text = serde_json::from_str::<Value>(&body).unwrap()["result"]["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Réparer le build") && text.contains("claude-code"), "{text}");
        {
            let store = server.store.lock().unwrap();
            assert_eq!(store.sessions.len(), 1, "one connection, one session");
            let tools: Vec<&str> = store.sessions[0].events.iter().map(|e| e.tool.as_str()).collect();
            assert_eq!(tools, ["session_note", "search_in_project", "get_session_context"]);
            assert_eq!(store.sessions[0].title.as_deref(), Some("Réparer le build"));
        }
        // Closing the connection ends the session.
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(stream, "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer secret\r\n{with_session}\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(server.store.lock().unwrap().sessions[0].ended.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_commit_commands_are_offered_as_prompts_and_the_commit_tools_are_listed() {
        let (_server, port) = running();
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list"}"#);
        let list: Value = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = list["result"]["prompts"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
        assert_eq!(names, ["commit", "commit-all", "commit-by-context"]);
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"commit","arguments":{"mode":"context"}}}"#);
        let got: Value = serde_json::from_str(&body).unwrap();
        let text = got["result"]["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("“context” mode") && text.contains("Co-authored-by"), "{text}");
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{"name":"nope"}}"#);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"]["code"], -32602);
        let (_, body) = post(port, Some("secret"), "", r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#);
        let tools: Value = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(["git_commit_plan", "git_diff", "git_commit"].iter().all(|n| names.contains(n)), "{names:?}");
    }
}
