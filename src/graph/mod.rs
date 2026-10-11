//! The context graph: a map of the project (files, what they define, how they lean on each other,
//! how they group) kept in `.atelier/graph`, for an AI to read in place of the files. It is made
//! from the syntax of the code ([`extract`]), with no AI, and kept up to date for as long as the
//! user leaves it on: a thread of its own looks at the files, extracts again those that changed
//! and builds the graph again, which takes a fraction of a second.
//!
//! Files written in `.atelier/graph/`: `graph.json` (node-link), `GRAPH_REPORT.md` (the summary
//! to read first) and `cache.json` (what each file said, so that a restart reads nothing again).
//! The AIs reach it through the MCP tools `graph_overview`, `graph_query`, `graph_node` and
//! `graph_path` ([`query`]).

pub mod build;
pub mod extract;
pub mod query;
pub mod report;

use build::Graph;
use extract::{Facts, MAX_SOURCE};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How often the files are looked at for changes, besides a save in the IDE.
const RESCAN: Duration = Duration::from_secs(10);
/// A project with more files than this is not mapped: the map would not be one.
const MAX_FILES: usize = 20_000;
const CACHE_VERSION: u32 = 1;

static CURRENT: RwLock<Option<Arc<Graph>>> = RwLock::new(None);

/// The graph of the project, when it is on.
pub fn current() -> Option<Arc<Graph>> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn publish(graph: Option<Arc<Graph>>) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = graph;
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// What the interface shows of the graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub busy: bool,
    pub files: usize,
    pub symbols: usize,
    pub groups: usize,
    /// When it was last built, in milliseconds since the epoch; 0 before the first time.
    pub updated: u64,
    pub error: Option<String>,
}

enum Msg {
    /// A file changed: look now.
    Rescan,
    /// Read every file again.
    Regenerate,
    Stop,
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    version: u32,
    files: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    mtime: u64,
    size: u64,
    facts: Facts,
}

pub fn folder(root: &Path) -> PathBuf {
    crate::atelier::dir(root).join("graph")
}

pub struct Service {
    root: PathBuf,
    sender: Option<Sender<Msg>>,
    status: Arc<Mutex<Status>>,
    /// The user has said yes or no for this project (read once, not on every frame).
    decided: bool,
    /// The project looks like code worth a graph: a repository or a project file.
    offerable: bool,
    /// "Later": not offered again until the project is opened again.
    postponed: bool,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            sender: None,
            status: Arc::default(),
            decided: true,
            offerable: false,
            postponed: false,
        }
    }
}

impl Service {
    /// The IDE shows another project: the graph of the previous one is dropped, and this one's
    /// is started if the user turned it on there.
    pub fn set_root(&mut self, root: &Path, wake: Arc<dyn Fn() + Send + Sync>) {
        self.stop();
        self.root = root.to_path_buf();
        let setting = crate::atelier::graph_setting(root);
        self.decided = setting.is_some();
        self.postponed = false;
        self.offerable = [".git", "Cargo.toml", "package.json", "tsconfig.json"]
            .iter()
            .any(|f| root.join(f).exists());
        if setting == Some(true) {
            self.start(wake);
        }
    }

    fn start(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let (sender, receiver) = mpsc::channel();
        let (root, status) = (self.root.clone(), self.status.clone());
        *status.lock().unwrap_or_else(|e| e.into_inner()) = Status::default();
        let spawned = std::thread::Builder::new()
            .name("graph".into())
            .spawn(move || run(&root, &receiver, &status, &*wake));
        if spawned.is_ok() {
            self.sender = Some(sender);
        }
    }

    fn stop(&mut self) {
        // Only a service that was running owns the published graph: another one (an IDE that never
        // turned it on) must not clear it.
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Msg::Stop);
            publish(None);
            *self.status.lock().unwrap_or_else(|e| e.into_inner()) = Status::default();
        }
    }

    pub fn enabled(&self) -> bool {
        self.sender.is_some()
    }

    /// Whether to offer the graph now: a project of code the user has not answered for.
    pub fn should_offer(&self) -> bool {
        self.offerable && !self.decided && !self.postponed && !self.enabled()
    }

    /// "Later": the offer goes away until the project is opened again.
    pub fn postpone(&mut self) {
        self.postponed = true;
    }

    /// Turns it on for the project (and remembers it there), and builds it.
    pub fn enable(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        crate::atelier::set_graph_setting(&self.root, true);
        crate::atelier::ensure_ignored(&self.root, "graph/");
        self.decided = true;
        if !self.enabled() {
            self.start(wake);
        }
    }

    /// Turns it off for the project: it is no longer kept up to date nor offered to the AIs.
    pub fn disable(&mut self) {
        crate::atelier::set_graph_setting(&self.root, false);
        self.decided = true;
        self.stop();
    }

    /// The user does not want it and does not want to be asked again.
    pub fn decline(&mut self) {
        crate::atelier::set_graph_setting(&self.root, false);
        self.decided = true;
    }

    /// Reads every file again.
    pub fn regenerate(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(Msg::Regenerate);
        }
    }

    /// A file of the project changed in the IDE.
    pub fn file_changed(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(Msg::Rescan);
        }
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The files of the project the graph reads: path from the root, path, modification time and size.
fn scan(root: &Path) -> Vec<(String, PathBuf, u64, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(children) = crate::project::searchable_children(&dir) else {
            continue;
        };
        for child in children {
            let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if child.is_dir() {
                // Hidden folders (`.atelier`, `.github`…) are not the code.
                if !name.starts_with('.') && !child.is_symlink() {
                    stack.push(child);
                }
                continue;
            }
            if child
                .extension()
                .and_then(|e| e.to_str())
                .and_then(extract::language_of)
                .is_none()
            {
                continue;
            }
            let Ok(meta) = fs::metadata(&child) else {
                continue;
            };
            if meta.len() > MAX_SOURCE {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            let rel = child
                .strip_prefix(root)
                .unwrap_or(&child)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, child, mtime, meta.len()));
            if out.len() >= MAX_FILES {
                return out;
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn load_cache(root: &Path) -> Cache {
    fs::read(folder(root).join("cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or_else(|| Cache {
            version: CACHE_VERSION,
            files: BTreeMap::new(),
        })
}

fn write_outputs(
    root: &Path,
    graph: &Graph,
    cache: &Cache,
    cache_changed: bool,
) -> std::io::Result<()> {
    let dir = folder(root);
    fs::create_dir_all(&dir)?;
    crate::atelier::ensure_ignored(root, "graph/");
    fs::write(
        dir.join("graph.json"),
        serde_json::to_vec(&report::json(graph))?,
    )?;
    fs::write(dir.join("GRAPH_REPORT.md"), report::markdown(graph))?;
    if cache_changed {
        fs::write(dir.join("cache.json"), serde_json::to_vec(cache)?)?;
    }
    Ok(())
}

/// Looks at the files, extracts again what changed and builds the graph again if anything did.
/// Whether it did.
fn update(
    root: &Path,
    cache: &mut Cache,
    force: bool,
    status: &Mutex<Status>,
    wake: &(dyn Fn() + Send + Sync),
) -> bool {
    let files = scan(root);
    let present: std::collections::HashSet<&str> = files.iter().map(|f| f.0.as_str()).collect();
    let stale: Vec<&(String, PathBuf, u64, u64)> = files
        .iter()
        .filter(|(rel, _, mtime, size)| {
            force
                || cache
                    .files
                    .get(rel)
                    .is_none_or(|e| e.mtime != *mtime || e.size != *size)
        })
        .collect();
    let removed: Vec<String> = cache
        .files
        .keys()
        .filter(|k| !present.contains(k.as_str()))
        .cloned()
        .collect();
    if stale.is_empty() && removed.is_empty() && current().is_some() {
        return false;
    }
    let _task = crate::tasks::start(tr!("context_graph_updating"));
    status.lock().unwrap_or_else(|e| e.into_inner()).busy = true;
    wake();
    for rel in &removed {
        cache.files.remove(rel);
    }
    for (rel, path, mtime, size) in &stale {
        let language = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(extract::language_of)
            .unwrap_or("js");
        // A file that cannot be read as text says nothing.
        let facts = fs::read_to_string(path)
            .map(|text| extract::extract(language, &text))
            .unwrap_or_default();
        cache.files.insert(
            rel.clone(),
            Entry {
                mtime: *mtime,
                size: *size,
                facts,
            },
        );
    }
    let facts: BTreeMap<String, Facts> = cache
        .files
        .iter()
        .map(|(k, v)| (k.clone(), v.facts.clone()))
        .collect();
    let mut graph = build::build(root, &facts, now());
    for node in graph.nodes.iter_mut().filter(|n| n.kind == "file") {
        node.size = cache.files.get(&node.file).map_or(0, |e| e.size);
    }
    let written = write_outputs(root, &graph, cache, true);
    let mut state = status.lock().unwrap_or_else(|e| e.into_inner());
    *state = Status {
        busy: false,
        files: report::files(&graph),
        symbols: report::symbols(&graph),
        groups: graph.communities.len(),
        updated: graph.generated,
        error: written
            .err()
            .map(|e| trf!("cannot_write_atelier_graph", e = e)),
    };
    drop(state);
    publish(Some(Arc::new(graph)));
    wake();
    true
}

fn run(
    root: &Path,
    messages: &mpsc::Receiver<Msg>,
    status: &Mutex<Status>,
    wake: &(dyn Fn() + Send + Sync),
) {
    let mut cache = load_cache(root);
    let mut force = false;
    loop {
        update(root, &mut cache, std::mem::take(&mut force), status, wake);
        match messages.recv_timeout(RESCAN) {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Regenerate) => force = true,
            Ok(Msg::Rescan) | Err(RecvTimeoutError::Timeout) => {}
        }
        // Several messages at once are one look.
        while let Ok(message) = messages.try_recv() {
            match message {
                Msg::Stop => return,
                Msg::Regenerate => force = true,
                Msg::Rescan => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "../../test/graph/mod/tests.rs"]
mod tests;
