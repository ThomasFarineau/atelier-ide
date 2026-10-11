//! Go to Symbol (Ctrl+N): the classes, functions and other definitions of the project, read from
//! the syntax of its files ([`crate::graph::extract`], and the types of Java files) in the
//! background. What a file defines is kept between searches and read again only when it changed.

use crate::graph::extract;
use regex::Regex;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, mpsc},
    time::{Duration, Instant, SystemTime},
};

/// The index is made again when a search starts after this long.
const STALE: Duration = Duration::from_secs(20);
/// Results shown at most.
pub const LIMIT: usize = 80;

static JAVA_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:(?:public|protected|private|abstract|final|static|sealed|non-sealed)\s+)*(class|interface|enum|record|@interface)\s+([A-Za-z_$][\w$]*)")
        .expect("regex")
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// Qualified by its type for a method (`Counter::increment`).
    pub name: String,
    pub kind: String,
    pub path: PathBuf,
    /// 0-based.
    pub line: usize,
}

/// What a file defines: (name, kind, 0-based line).
type Defined = Vec<(String, String, usize)>;

/// The definitions of a file, from its syntax.
pub fn defined(path: &Path, text: &str) -> Defined {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "java" {
        return JAVA_TYPE
            .captures_iter(text)
            .filter_map(|c| {
                let name = c.get(2)?;
                let kind = match c.get(1)?.as_str() {
                    "@interface" => "interface",
                    other => other,
                };
                let line = text[..name.start()].matches('\n').count();
                Some((name.as_str().to_string(), kind.to_string(), line))
            })
            .collect();
    }
    let Some(language) = extract::language_of(&extension) else {
        return Vec::new();
    };
    // Markdown headings are no symbols.
    if language == "md" {
        return Vec::new();
    }
    extract::extract(language, text)
        .symbols
        .into_iter()
        .map(|s| (s.name, s.kind, s.line.saturating_sub(1)))
        .collect()
}

/// The symbols matching `query`, the best first: a fuzzy match on the name, the last segment of
/// a qualified name counting most.
pub fn search<'a>(symbols: &'a [Symbol], query: &str) -> Vec<&'a Symbol> {
    let mut found: Vec<(i32, &Symbol)> = symbols
        .iter()
        .filter_map(|s| {
            let short = s.name.rsplit("::").next().unwrap_or(&s.name);
            let score = crate::languages::fuzzy_score(query, short)
                .map(|n| n + 20)
                .or_else(|| crate::languages::fuzzy_score(query, &s.name))?;
            // Types before functions, as IntelliJ's Go to Class.
            let kind = match s.kind.as_str() {
                "class" | "struct" | "interface" | "trait" | "enum" | "record" | "type" => 3,
                _ => 0,
            };
            let exact = i32::from(short.eq_ignore_ascii_case(query)) * 50;
            Some((score + kind + exact - short.len() as i32 / 4, s))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().take(LIMIT).map(|(_, s)| s).collect()
}

type Cache = HashMap<PathBuf, (SystemTime, Arc<Defined>)>;

#[derive(Default)]
pub struct Index {
    cache: Arc<Mutex<Cache>>,
    pending: Option<mpsc::Receiver<Vec<Symbol>>>,
    pub symbols: Arc<Vec<Symbol>>,
    built: Option<(PathBuf, Instant)>,
}

impl Index {
    /// Whether the symbols are being read.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Reads the symbols of `files` again in the background, unless they were just read for
    /// `root`. `wake` is called once they are.
    pub fn refresh(
        &mut self,
        root: &Path,
        files: impl FnOnce() -> Vec<PathBuf> + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) {
        let fresh = self
            .built
            .as_ref()
            .is_some_and(|(r, at)| r == root && at.elapsed() < STALE);
        if self.pending.is_some() || fresh {
            return;
        }
        if self.built.as_ref().is_some_and(|(r, _)| r != root) {
            self.symbols = Arc::default();
            self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
        self.built = Some((root.to_path_buf(), Instant::now()));
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let cache = self.cache.clone();
        std::thread::spawn(move || {
            let mut symbols = Vec::new();
            for path in files() {
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if meta.len() > extract::MAX_SOURCE {
                    continue;
                }
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let known = cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&path)
                    .filter(|(at, _)| *at == modified)
                    .map(|(_, d)| d.clone());
                let found = known.unwrap_or_else(|| {
                    let found = Arc::new(
                        std::fs::read_to_string(&path)
                            .map(|text| defined(&path, &text))
                            .unwrap_or_default(),
                    );
                    cache
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(path.clone(), (modified, found.clone()));
                    found
                });
                symbols.extend(found.iter().map(|(name, kind, line)| Symbol {
                    name: name.clone(),
                    kind: kind.clone(),
                    path: path.clone(),
                    line: *line,
                }));
            }
            let _ = tx.send(symbols);
            wake();
        });
    }

    /// Takes the symbols once they are read.
    pub fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(symbols) => {
                self.symbols = Arc::new(symbols);
                self.pending = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

#[cfg(test)]
#[path = "../../test/app/symbols/tests.rs"]
mod tests;
