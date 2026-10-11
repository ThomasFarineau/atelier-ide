//! What the AI command lines installed on the computer have used, read from the files they keep
//! on it: the tokens of each answer and, for Codex, the share of its usage limits. Nothing is asked
//! of their servers, so the figures are those of this computer only.
//!
//! - Claude Code: `~/.claude/projects/<project>/<session>.jsonl` (or `$CLAUDE_CONFIG_DIR`), one
//!   line per message, the answers with their `usage` (a message cut in several lines repeats it).
//! - Codex: `~/.codex/sessions/<y>/<m>/<d>/rollout-*.jsonl` (or `$CODEX_HOME`), whose
//!   `token_count` events hold the tokens of the last turn and the usage limits.
//! - Gemini CLI: `~/.gemini/tmp/<project hash>/chats/session-*.json`, the messages of the model
//!   with their `tokens`.
//!
//! The files are read again only when they change, in the background.

use super::agents::Agent;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

const DAY_MS: u64 = 24 * 3600 * 1000;
/// How far back the usage is read.
const MONTH_MS: u64 = 30 * DAY_MS;

/// Tokens, split as the providers bill them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    /// Read without cache (Claude: cache writes included).
    pub input: u64,
    /// Read from the cache.
    pub cached: u64,
    /// Written, reasoning included.
    pub output: u64,
}

impl Tokens {
    pub fn total(self) -> u64 {
        self.input + self.cached + self.output
    }

    fn add(&mut self, other: Tokens) {
        self.input += other.input;
        self.cached += other.cached;
        self.output += other.output;
    }
}

/// One answer of a model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Milliseconds since 1970.
    pub at: u64,
    pub model: String,
    pub tokens: Tokens,
    /// The message, for those written several times.
    pub id: Option<String>,
}

/// A usage limit of the subscription: the share used of what a sliding window allows.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub window_minutes: u64,
    pub used_percent: f64,
    /// Seconds since 1970.
    pub resets_at: Option<u64>,
}

/// What one file of an AI says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileUsage {
    /// The folder the AI worked in.
    pub cwd: Option<String>,
    pub records: Vec<Record>,
    /// The last limits seen, and when.
    pub limits: Option<(u64, Vec<Limit>, Option<String>)>,
}

/// The usage of one AI.
#[derive(Clone, Debug, Default)]
pub struct AgentUsage {
    /// The program, when it is on the PATH.
    pub program: Option<PathBuf>,
    pub version: Option<String>,
    /// Whether the project declares the MCP server of the IDE for it.
    pub connected: bool,
    pub day: Tokens,
    pub week: Tokens,
    pub month: Tokens,
    /// This project, over 30 days.
    pub project: Tokens,
    /// Answers over 30 days.
    pub answers: usize,
    /// Sessions over 30 days.
    pub sessions: usize,
    /// Total tokens per model over 30 days, the most used first.
    pub models: Vec<(String, u64)>,
    pub last: Option<u64>,
    /// When the limits were read, the limits and the plan.
    pub limits: Option<(u64, Vec<Limit>, Option<String>)>,
}

pub const AGENTS: [Agent; 3] = [Agent::Claude, Agent::Codex, Agent::Gemini];

/// Milliseconds since 1970 of an RFC 3339 date (`2026-10-10T13:35:27.240Z`, or with an offset).
pub fn parse_time(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[13] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &text[19..];
    let mut ms = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        let first: String = fraction[..digits]
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        ms = first.parse::<i64>().ok()?;
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let rest = rest.get(1..)?;
            let hours = rest.get(0..2)?.parse::<i64>().ok()?;
            let minutes = rest
                .get(3..5)
                .and_then(|m| m.parse::<i64>().ok())
                .unwrap_or(0);
            sign * (hours * 60 + minutes) * 60
        }
    };
    // Days from 1970-01-01 of a civil date (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86400 + h * 3600 + mi * 60 + s - offset;
    u64::try_from(secs * 1000 + ms).ok()
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// The answer a line of a Claude Code session holds, if it is one.
pub fn claude_line(line: &str) -> Option<Record> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let message = value.get("message")?;
    let usage = message.get("usage")?;
    let model = message.get("model")?.as_str()?;
    // Answers written by Claude Code itself (errors, interruptions) cost nothing.
    if model.starts_with('<') {
        return None;
    }
    Some(Record {
        at: parse_time(value.get("timestamp")?.as_str()?)?,
        model: model.to_string(),
        tokens: Tokens {
            input: number(usage, "input_tokens") + number(usage, "cache_creation_input_tokens"),
            cached: number(usage, "cache_read_input_tokens"),
            output: number(usage, "output_tokens"),
        },
        id: message
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// A Claude Code session.
pub fn claude_file(text: impl BufRead) -> FileUsage {
    let mut usage = FileUsage::default();
    for line in text.lines().map_while(Result::ok) {
        if usage.cwd.is_none() && line.contains("\"cwd\"") {
            usage.cwd = serde_json::from_str::<Value>(&line)
                .ok()
                .and_then(|v| Some(v.get("cwd")?.as_str()?.to_string()));
        }
        if let Some(record) = claude_line(&line) {
            // The blocks of one answer are written on lines of their own, each with the usage.
            if record.id.is_some() && usage.records.last().is_some_and(|r| r.id == record.id) {
                continue;
            }
            usage.records.push(record);
        }
    }
    usage
}

fn limit(value: &Value) -> Option<Limit> {
    Some(Limit {
        window_minutes: value.get("window_minutes")?.as_u64()?,
        used_percent: value.get("used_percent")?.as_f64()?,
        resets_at: value.get("resets_at").and_then(Value::as_u64),
    })
}

/// A Codex session.
pub fn codex_file(text: impl BufRead) -> FileUsage {
    let mut usage = FileUsage::default();
    let mut model = String::from("codex");
    for line in text.lines().map_while(Result::ok) {
        let interesting = ["\"session_meta\"", "\"turn_context\"", "\"token_count\""];
        if !interesting.iter().any(|k| line.contains(k)) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = &value["payload"];
        match value["type"].as_str() {
            Some("session_meta") => {
                usage.cwd = payload["cwd"].as_str().map(str::to_string);
            }
            Some("turn_context") => {
                if let Some(name) = payload["model"].as_str() {
                    model = name.to_string();
                }
            }
            Some("event_msg") if payload["type"] == "token_count" => {
                let Some(at) = value["timestamp"].as_str().and_then(parse_time) else {
                    continue;
                };
                let last = &payload["info"]["last_token_usage"];
                if last.is_object() {
                    let input = number(last, "input_tokens");
                    let cached = number(last, "cached_input_tokens").min(input);
                    usage.records.push(Record {
                        at,
                        model: model.clone(),
                        tokens: Tokens {
                            input: input - cached,
                            cached,
                            output: number(last, "output_tokens"),
                        },
                        id: None,
                    });
                }
                let limits = &payload["rate_limits"];
                let found: Vec<Limit> = ["primary", "secondary"]
                    .iter()
                    .filter_map(|k| limit(&limits[*k]))
                    .collect();
                if !found.is_empty() {
                    let plan = limits["plan_type"].as_str().map(str::to_string);
                    usage.limits = Some((at, found, plan));
                }
            }
            _ => {}
        }
    }
    usage
}

/// A Gemini CLI session.
pub fn gemini_file(text: &str) -> FileUsage {
    let mut usage = FileUsage::default();
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return usage;
    };
    for message in value["messages"].as_array().into_iter().flatten() {
        let tokens = &message["tokens"];
        if !tokens.is_object() {
            continue;
        }
        let Some(at) = message["timestamp"].as_str().and_then(parse_time) else {
            continue;
        };
        let input = number(tokens, "input");
        let cached = number(tokens, "cached").min(input);
        usage.records.push(Record {
            at,
            model: message["model"].as_str().unwrap_or("gemini").to_string(),
            tokens: Tokens {
                input: input - cached,
                cached,
                output: number(tokens, "output") + number(tokens, "thoughts"),
            },
            id: message["id"].as_str().map(str::to_string),
        });
    }
    usage
}

/// The folder Gemini CLI keeps the sessions of a project in is named after this.
pub fn gemini_project_hash(root: &Path) -> String {
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Where an AI keeps its sessions.
fn data_folder(agent: Agent) -> PathBuf {
    let from = |var: &str, default: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(default))
    };
    match agent {
        Agent::Claude => from("CLAUDE_CONFIG_DIR", ".claude").join("projects"),
        Agent::Codex => from("CODEX_HOME", ".codex").join("sessions"),
        Agent::Gemini => home().join(".gemini").join("tmp"),
    }
}

/// The session files of `agent` changed in the last 30 days.
fn session_files(agent: Agent, since: SystemTime) -> Vec<(PathBuf, SystemTime, u64)> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth > 0 {
                    walk(&path, depth - 1, out);
                }
            } else {
                out.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    walk(&data_folder(agent), 4, &mut paths);
    paths
        .into_iter()
        .filter(|p| match agent {
            Agent::Gemini => {
                p.extension().is_some_and(|e| e == "json")
                    && p.parent()
                        .and_then(Path::file_name)
                        .is_some_and(|n| n == "chats")
            }
            _ => p.extension().is_some_and(|e| e == "jsonl"),
        })
        .filter_map(|p| {
            let meta = fs::metadata(&p).ok()?;
            let modified = meta.modified().ok()?;
            (modified >= since).then_some((p, modified, meta.len()))
        })
        .collect()
}

fn read_file(agent: Agent, path: &Path) -> FileUsage {
    let Ok(file) = fs::File::open(path) else {
        return FileUsage::default();
    };
    match agent {
        Agent::Claude => claude_file(BufReader::new(file)),
        Agent::Codex => codex_file(BufReader::new(file)),
        Agent::Gemini => fs::read_to_string(path)
            .map(|t| gemini_file(&t))
            .unwrap_or_default(),
    }
}

/// The program of `agent` on the PATH.
fn program(agent: Agent) -> Option<PathBuf> {
    let name = agent.id();
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd", "bat", "ps1"]
            .iter()
            .map(|ext| format!("{name}.{ext}"))
            .collect()
    } else {
        vec![name.to_string()]
    };
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// What `program --version` prints, its first line without the name of the program.
fn version(program: &Path) -> Option<String> {
    let script = program
        .extension()
        .is_some_and(|e| e == "cmd" || e == "bat" || e == "ps1");
    let mut command = if script && cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        command.arg("/C").arg(program);
        command
    } else {
        std::process::Command::new(program)
    };
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    clean_version(line)
}

/// `2.1.296 (Claude Code)` or `codex-cli 0.158.0` → the version.
pub fn clean_version(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()) && w.contains('.'))
        .map(|w| w.trim_end_matches([',', ')']).to_string())
}

/// Sums what the files of one AI say, as seen at `now` for the project `root`.
pub fn summarize(
    files: &[Arc<FileUsage>],
    root: &Path,
    project_hash: Option<&str>,
    now: u64,
) -> AgentUsage {
    let mut usage = AgentUsage::default();
    let root = root.to_string_lossy();
    let same = |cwd: &str| {
        let normalize = |s: &str| {
            s.trim_start_matches(r"\\?\")
                .trim_end_matches(['/', '\\'])
                .replace('\\', "/")
                .to_lowercase()
        };
        normalize(cwd) == normalize(&root)
    };
    let mut seen = HashSet::new();
    let mut models: BTreeMap<String, u64> = BTreeMap::new();
    for (file, hash) in files.iter().map(|f| (f, project_hash)) {
        let in_project = file.cwd.as_deref().is_some_and(same)
            || hash.is_some_and(|h| file.cwd.as_deref() == Some(h));
        let mut counted = false;
        for record in &file.records {
            let age = now.saturating_sub(record.at);
            if age > MONTH_MS {
                continue;
            }
            if let Some(id) = &record.id
                && !seen.insert(id.clone())
            {
                continue;
            }
            counted = true;
            usage.answers += 1;
            usage.month.add(record.tokens);
            if age <= 7 * DAY_MS {
                usage.week.add(record.tokens);
            }
            if age <= DAY_MS {
                usage.day.add(record.tokens);
            }
            if in_project {
                usage.project.add(record.tokens);
            }
            *models.entry(record.model.clone()).or_default() += record.tokens.total();
            usage.last = usage.last.max(Some(record.at));
        }
        usage.sessions += usize::from(counted);
        if let Some(limits) = &file.limits
            && usage.limits.as_ref().is_none_or(|(at, ..)| limits.0 > *at)
        {
            usage.limits = Some(limits.clone());
        }
    }
    usage.models = models.into_iter().collect();
    usage
        .models
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    usage
}

/// A file read: when it was changed, its size, and what it says.
type Cached = (SystemTime, u64, Arc<FileUsage>);

/// Reads the usage in the background, keeping what it read of the files that did not change.
#[derive(Default)]
pub struct Scanner {
    cache: Arc<Mutex<HashMap<PathBuf, Cached>>>,
    versions: Arc<Mutex<HashMap<PathBuf, Option<String>>>>,
    pending: Option<mpsc::Receiver<Vec<(Agent, AgentUsage)>>>,
    pub report: Option<(u64, Vec<(Agent, AgentUsage)>)>,
}

impl Scanner {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Starts a new reading for the project `root`; `connected` tells whether the project declares
    /// the server for an AI. `repaint` is called once it is done.
    pub fn refresh(
        &mut self,
        root: &Path,
        connected: impl Fn(Agent) -> bool,
        repaint: impl Fn() + Send + 'static,
    ) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let (cache, versions) = (self.cache.clone(), self.versions.clone());
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        let connected: Vec<bool> = AGENTS.iter().map(|a| connected(*a)).collect();
        std::thread::spawn(move || {
            let now = SystemTime::now();
            let since = now - std::time::Duration::from_millis(MONTH_MS);
            let now_ms = now
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            let mut report = Vec::new();
            for (agent, connected) in AGENTS.into_iter().zip(connected) {
                let mut files = Vec::new();
                for (path, modified, len) in session_files(agent, since) {
                    let known = cache
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(&path)
                        .filter(|(m, l, _)| *m == modified && *l == len)
                        .map(|(.., usage)| usage.clone());
                    let usage = known.unwrap_or_else(|| {
                        let mut usage = read_file(agent, &path);
                        // Gemini does not write the folder: the project is the name of its folder.
                        if agent == Agent::Gemini {
                            usage.cwd = path
                                .parent()
                                .and_then(Path::parent)
                                .and_then(Path::file_name)
                                .map(|n| n.to_string_lossy().into_owned());
                        }
                        let usage = Arc::new(usage);
                        cache
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(path, (modified, len, usage.clone()));
                        usage
                    });
                    files.push(usage);
                }
                let hash = (agent == Agent::Gemini).then(|| gemini_project_hash(&root));
                let mut usage = summarize(&files, &root, hash.as_deref(), now_ms);
                usage.program = program(agent);
                usage.version = usage.program.as_ref().and_then(|p| {
                    versions
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .entry(p.clone())
                        .or_insert_with(|| version(p))
                        .clone()
                });
                usage.connected = connected;
                report.push((agent, usage));
            }
            let _ = tx.send(report);
            repaint();
        });
    }

    /// Takes the result of the reading when it is done.
    pub fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(report) => {
                self.report = Some((super::sessions::now(), report));
                self.pending = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

/// `1234567` → `1,2 M`, in the style of the interface language.
pub fn short_count(n: u64) -> String {
    let (value, unit) = match n {
        0..1_000 => return n.to_string(),
        1_000..1_000_000 => (n as f64 / 1e3, "k"),
        1_000_000..1_000_000_000 => (n as f64 / 1e6, "M"),
        _ => (n as f64 / 1e9, "G"),
    };
    let text = if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    let text = text.trim_end_matches(".0").to_string();
    let text = if crate::i18n::current().starts_with("fr") {
        text.replace('.', ",")
    } else {
        text
    };
    format!("{text} {unit}")
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/mcp/usage/tests.rs"]
mod tests;
