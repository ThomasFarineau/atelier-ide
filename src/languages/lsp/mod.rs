//! A client of the Language Server Protocol, for the languages that have a server.
//!
//! The server is a child process spoken to over its standard input and output, with JSON-RPC
//! messages framed by a `Content-Length` header. Requests are answered on a reader thread,
//! which runs the callback given with the request; notifications sent before the server is
//! ready are kept and sent once it is.

pub mod convert;
pub mod extras;
pub mod provider;

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

type Callback = Box<dyn FnOnce(Result<Value, Value>) + Send>;

/// A problem reported by the server, with positions as the protocol gives them.
#[derive(Clone, Debug)]
pub struct ServerDiagnostic {
    pub start: (usize, usize),
    pub end: (usize, usize),
    /// 1 error, 2 warning, 3 information, 4 hint.
    pub severity: u8,
    pub message: String,
    pub source: String,
    /// The diagnostic as the server sent it, to give back with a code action request.
    pub raw: Value,
}

#[derive(Default)]
struct Shared {
    callbacks: HashMap<u64, Callback>,
    diagnostics: HashMap<String, Vec<ServerDiagnostic>>,
    /// Bumped each time diagnostics arrive.
    generation: u64,
    ready: bool,
    /// Why the handshake failed.
    error: Option<String>,
    /// Notifications waiting for the server to be ready.
    queued: Vec<Value>,
    /// The last lines the server wrote on its standard error.
    log: Vec<String>,
}

pub struct Client {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    shared: Arc<Mutex<Shared>>,
    next_id: AtomicU64,
    exited: Arc<AtomicBool>,
}

/// How to launch a server.
#[derive(Clone, Debug)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The `initializationOptions` of the initialize request.
    pub options: Value,
}

impl Client {
    /// Starts the server and begins the handshake. Returns at once: the server is ready when
    /// [`Client::ready`] says so.
    pub fn start(
        launch: &Launch,
        root: &Path,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>> {
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .with_context(|| trf!("lsp.cannot_start", program = launch.program.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!(tr!("lsp.no_standard_input")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!(tr!("lsp.no_standard_output")))?;
        let stderr = child.stderr.take();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let exited = Arc::new(AtomicBool::new(false));
        let client = Arc::new(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            shared: shared.clone(),
            next_id: AtomicU64::new(1),
            exited: exited.clone(),
        });

        // Reader: answers and notifications from the server.
        {
            let shared = shared.clone();
            let exited = exited.clone();
            let weak = Arc::downgrade(&client);
            let repaint = repaint.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(message)) = read_message(&mut reader) {
                    handle_message(message, &shared, weak.upgrade().as_deref());
                    repaint();
                }
                exited.store(true, Ordering::SeqCst);
                // Whoever waits for an answer will not get it.
                let callbacks: Vec<Callback> = shared
                    .lock()
                    .map(|mut s| s.callbacks.drain().map(|(_, c)| c).collect())
                    .unwrap_or_default();
                for callback in callbacks {
                    callback(Err(json!({"message": tr!("lsp.server_stopped_lower")})));
                }
                repaint();
            });
        }
        if let Some(stderr) = stderr {
            let shared = shared.clone();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut s) = shared.lock() {
                        s.log.push(line);
                        let excess = s.log.len().saturating_sub(40);
                        s.log.drain(..excess);
                    }
                }
            });
        }

        let root_uri = path_to_uri(root);
        let name = root.file_name().map_or(tr!("lsp.project").into(), |n| {
            n.to_string_lossy().into_owned()
        });
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "Atelier"},
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": name}],
            "initializationOptions": launch.options,
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "textDocument": {
                    "synchronization": {"didSave": false, "willSave": false},
                    "completion": {
                        "completionItem": {
                            "snippetSupport": false,
                            "documentationFormat": ["markdown", "plaintext"],
                            "labelDetailsSupport": true
                        },
                        "contextSupport": true
                    },
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "definition": {"linkSupport": true},
                    "codeAction": {
                        "codeActionLiteralSupport": {"codeActionKind": {"valueSet": ["quickfix", "refactor", "source"]}}
                    },
                    "signatureHelp": {"signatureInformation": {"documentationFormat": ["markdown", "plaintext"], "parameterInformation": {"labelOffsetSupport": true}, "activeParameterSupport": true}},
                    "inlayHint": {},
                    "callHierarchy": {},
                    "typeHierarchy": {},
                    "publishDiagnostics": {"relatedInformation": false}
                },
                "workspace": {"workspaceFolders": true, "configuration": false}
            }
        });
        let weak = Arc::downgrade(&client);
        client.send_request(
            "initialize",
            params,
            Box::new(move |result| {
                let Some(client) = weak.upgrade() else { return };
                if let Err(error) = result {
                    let message = error["message"]
                        .as_str()
                        .unwrap_or(tr!("lsp.initialization_refused"))
                        .to_string();
                    if let Ok(mut s) = client.shared.lock() {
                        s.error = Some(message);
                    }
                    return;
                }
                let _ =
                    client.write(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
                let queued: Vec<Value> = client
                    .shared
                    .lock()
                    .map(|mut s| {
                        s.ready = true;
                        std::mem::take(&mut s.queued)
                    })
                    .unwrap_or_default();
                for message in queued {
                    let _ = client.write(&message);
                }
            }),
        )?;
        Ok(client)
    }

    pub fn ready(&self) -> bool {
        !self.exited() && self.shared.lock().is_ok_and(|s| s.ready)
    }

    /// The reason the server refused to start, once it did.
    pub fn failure(&self) -> Option<String> {
        self.shared.lock().ok().and_then(|s| s.error.clone())
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// The last lines the server printed on its error output, for the error message.
    pub fn log(&self) -> String {
        self.shared
            .lock()
            .map(|s| s.log.join("\n"))
            .unwrap_or_default()
    }

    fn write(&self, message: &Value) -> Result<()> {
        let body = serde_json::to_vec(message)?;
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!(tr!("lsp.input_locked")))?;
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        Ok(())
    }

    fn send_request(&self, method: &str, params: Value, callback: Callback) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.shared
            .lock()
            .map_err(|_| anyhow!(tr!("lsp.state_locked")))?
            .callbacks
            .insert(id, callback);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = self.write(&message) {
            let callback = self
                .shared
                .lock()
                .ok()
                .and_then(|mut s| s.callbacks.remove(&id));
            if let Some(callback) = callback {
                callback(Err(json!({"message": e.to_string()})));
            }
            return Err(e);
        }
        Ok(())
    }

    /// Sends a request once the server is ready; `callback` gets the result or the error.
    pub fn request(
        &self,
        method: &str,
        params: Value,
        callback: impl FnOnce(Result<Value, Value>) + Send + 'static,
    ) {
        if self.exited() {
            callback(Err(json!({"message": tr!("lsp.server_stopped")})));
            return;
        }
        let _ = self.send_request(method, params, Box::new(callback));
    }

    /// Sends a notification, or keeps it until the server is ready.
    pub fn notify(&self, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let queued = match self.shared.lock() {
            Ok(mut shared) if !shared.ready => {
                shared.queued.push(message.clone());
                true
            }
            _ => false,
        };
        if !queued {
            let _ = self.write(&message);
        }
    }

    /// Problems the server published for a document, and how many times they changed.
    pub fn diagnostics(&self, uri: &str) -> (Vec<ServerDiagnostic>, u64) {
        self.shared
            .lock()
            .map(|s| {
                (
                    s.diagnostics.get(uri).cloned().unwrap_or_default(),
                    s.generation,
                )
            })
            .unwrap_or_default()
    }

    pub fn generation(&self) -> u64 {
        self.shared.lock().map(|s| s.generation).unwrap_or(0)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Ask politely, then make sure the process is gone.
        let _ =
            self.write(&json!({"jsonrpc": "2.0", "id": 0, "method": "shutdown", "params": null}));
        let _ = self.write(&json!({"jsonrpc": "2.0", "method": "exit", "params": null}));
        if let Ok(mut child) = self.child.lock() {
            thread::sleep(Duration::from_millis(50));
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| anyhow!(tr!("lsp.header_without_length")))?;
    if length > 256 * 1024 * 1024 {
        bail!(tr!("lsp.message_too_large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn handle_message(message: Value, shared: &Arc<Mutex<Shared>>, client: Option<&Client>) {
    let id = message.get("id").and_then(Value::as_u64);
    let method = message.get("method").and_then(Value::as_str);
    match (id, method) {
        // The answer to one of our requests.
        (Some(id), None) => {
            let callback = shared.lock().ok().and_then(|mut s| s.callbacks.remove(&id));
            if let Some(callback) = callback {
                match message.get("error") {
                    Some(error) => callback(Err(error.clone())),
                    None => callback(Ok(message.get("result").cloned().unwrap_or(Value::Null))),
                }
            }
        }
        // A request from the server: it wants an answer, whatever it is.
        (Some(id), Some(method)) => {
            let result = match method {
                "workspace/configuration" => {
                    let count = message["params"]["items"].as_array().map_or(0, Vec::len);
                    json!(vec![Value::Null; count])
                }
                _ => Value::Null,
            };
            if let Some(client) = client {
                let _ = client.write(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
            }
        }
        (None, Some("textDocument/publishDiagnostics")) => {
            let params = &message["params"];
            let Some(uri) = params["uri"].as_str() else {
                return;
            };
            let list = params["diagnostics"]
                .as_array()
                .map(|items| items.iter().filter_map(parse_diagnostic).collect())
                .unwrap_or_default();
            if let Ok(mut s) = shared.lock() {
                s.diagnostics.insert(normalize_uri(uri), list);
                s.generation += 1;
            }
        }
        _ => {}
    }
}

fn position(value: &Value) -> Option<(usize, usize)> {
    Some((
        value["line"].as_u64()? as usize,
        value["character"].as_u64()? as usize,
    ))
}

fn parse_diagnostic(value: &Value) -> Option<ServerDiagnostic> {
    Some(ServerDiagnostic {
        start: position(&value["range"]["start"])?,
        end: position(&value["range"]["end"])?,
        severity: value["severity"].as_u64().unwrap_or(1) as u8,
        message: value["message"].as_str()?.to_string(),
        source: value["source"].as_str().unwrap_or("").to_string(),
        raw: value.clone(),
    })
}

// ---- Positions and addresses ---------------------------------------------------------------

/// Line and UTF-16 column of a byte offset, as the protocol counts.
pub fn to_position(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, text[start..offset].encode_utf16().count())
}

/// Byte offset of a line and UTF-16 column, clamped to the text.
pub fn from_position(text: &str, line: usize, column: usize) -> usize {
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let mut units = 0;
    for (i, c) in text[start..end].char_indices() {
        if units >= column {
            return start + i;
        }
        units += c.len_utf16();
    }
    end
}

/// The `file://` address of a path.
pub fn path_to_uri(path: &Path) -> String {
    let text = crate::project::simplify(path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~' | b':') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The path a `file://` address stands for.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = percent_decode(uri.strip_prefix("file://")?);
    // `/C:/Users/…` is `C:\Users\…` on Windows.
    let rest = match rest.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &rest[1..],
        _ => &rest[..],
    };
    Some(PathBuf::from(rest))
}

/// Servers may encode the drive colon or change its case: compare addresses in one form.
pub fn normalize_uri(uri: &str) -> String {
    let decoded = percent_decode(uri);
    let mut text = decoded.replace("%3A", ":").replace("%3a", ":");
    // `file:///C:/x` and `file:///c:/x` are the same place on Windows.
    if let Some(rest) = text.strip_prefix("file:///")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        let drive = rest[..1].to_ascii_lowercase();
        text = format!("file:///{drive}{}", &rest[1..]);
    }
    text
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[path = "../../../test/languages/lsp/mod/tests.rs"]
mod tests;
