//! `atelier <folder|file>`: the command line hands what it is given to the IDE that is already
//! open, or starts one.
//!
//! Every running IDE registers itself in a small file (its port, a secret and the project it
//! shows) and listens on a local socket. A new process that is given a path looks for the IDE
//! the path belongs to (the project that contains it, else the one used last), asks it to open
//! the path and exits; with none to ask, it starts normally. `-n` / `--new-window` skips the
//! asking, which is also how an IDE opens a project in a window of its own.

use crate::project::simplify;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    hash::{BuildHasher, Hasher},
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Set by tests to keep their registry apart from the real one.
const DIR_ENV: &str = "ATELIER_INSTANCES_DIR";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Entry {
    pid: u32,
    port: u16,
    token: String,
    /// The project the IDE shows.
    root: PathBuf,
    /// When the window was last in front, in milliseconds since the epoch.
    active_at: u64,
}

#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    open: PathBuf,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Where the IDE keeps what it has to remember between runs and between its windows.
pub fn data_dir() -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
    };
    base.join("Atelier")
}

/// Where the running IDEs are listed.
fn registry_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DIR_ENV) {
        return PathBuf::from(dir);
    }
    data_dir().join("instances")
}

/// An existing path of the command line, absolute.
fn absolute(path: &Path) -> PathBuf {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    simplify(&full.canonicalize().unwrap_or(full))
}

/// The folder or file the command line asks to open.
pub fn target() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .filter(|a| !a.to_string_lossy().starts_with('-'))
        .map(|a| absolute(Path::new(&a)))
        .find(|p| p.exists())
}

/// `-n` / `--new-window`: do not reuse a running IDE.
pub fn new_window() -> bool {
    std::env::args_os()
        .skip(1)
        .any(|a| a == "-n" || a == "--new-window")
}

fn entries() -> Vec<(PathBuf, Entry)> {
    let Ok(dir) = fs::read_dir(registry_dir()) else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|file| {
            let entry = serde_json::from_slice(&fs::read(file.path()).ok()?).ok()?;
            Some((file.path(), entry))
        })
        .collect()
}

/// How well an IDE suits `path`: the deeper the project that contains it the better.
fn score(entry: &Entry, path: &Path) -> usize {
    let root = simplify(&entry.root);
    if path.starts_with(&root) {
        root.components().count() + 1
    } else {
        0
    }
}

fn send(entry: &Entry, path: &Path) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, entry.port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
    let request = Request {
        token: entry.token.clone(),
        open: path.to_path_buf(),
    };
    let Ok(mut line) = serde_json::to_string(&request) else {
        return false;
    };
    line.push('\n');
    if stream.write_all(line.as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}

/// Gives `path` to a running IDE. False when there is none, and this process should open it.
pub fn forward(path: &Path) -> bool {
    let mut running = entries();
    running.sort_by_key(|(_, e)| std::cmp::Reverse((score(e, path), e.active_at)));
    for (file, entry) in running {
        if send(&entry, path) {
            return true;
        }
        // Nobody answers: the IDE is gone and left its file behind.
        let _ = fs::remove_file(file);
    }
    false
}

/// What the running IDE listens with, and what it has been asked.
pub struct Server {
    requests: mpsc::Receiver<PathBuf>,
    entry: Entry,
    file: PathBuf,
    in_front: bool,
}

/// Registers this IDE and starts listening. `wake` is called when a request arrives.
pub fn serve(root: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Option<Server> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let token = format!(
        "{:016x}{:016x}",
        std::hash::RandomState::new().build_hasher().finish(),
        now()
    );
    let entry = Entry {
        pid: std::process::id(),
        port,
        token,
        root: simplify(root),
        active_at: now(),
    };
    let dir = registry_dir();
    fs::create_dir_all(&dir).ok()?;
    let file = dir.join(format!("{}.json", entry.pid));
    let (sender, requests) = mpsc::channel();
    let secret = entry.token.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
            let mut line = String::new();
            let mut reader = BufReader::new(&stream);
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<Request>(&line) else {
                continue;
            };
            if request.token != secret {
                continue;
            }
            let _ = sender.send(request.open);
            wake();
            let _ = (&stream).write_all(b"ok\n");
        }
    });
    let server = Server {
        requests,
        entry,
        file,
        in_front: false,
    };
    server.write();
    Some(server)
}

impl Server {
    fn write(&self) {
        if let Ok(json) = serde_json::to_vec(&self.entry) {
            let _ = fs::write(&self.file, json);
        }
    }

    /// What other processes asked this IDE to open since the last call.
    pub fn poll(&self) -> Vec<PathBuf> {
        self.requests.try_iter().collect()
    }

    /// Tells the registry which project this IDE shows and whether its window is in front.
    pub fn update(&mut self, root: &Path, in_front: bool) {
        let root = simplify(root);
        let fronted = in_front && !self.in_front;
        self.in_front = in_front;
        if root != self.entry.root || fronted {
            self.entry.root = root;
            if in_front {
                self.entry.active_at = now();
            }
            self.write();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
    }
}

#[cfg(test)]
#[path = "../test/cli/tests.rs"]
mod tests;
