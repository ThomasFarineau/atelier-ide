//! The sessions of the AIs that use the MCP server. Each connection is a session of the project:
//! who connected, what it asked of the IDE, and the notes it left about its goal, decisions and
//! progress. They are kept in the project (`.atelier/sessions`), so that the context is the IDE's
//! and not the AI's: any AI can take a session up where the last one stopped, from the context the
//! IDE writes out ([`Store::markdown`]).
//!
//! The protocol only shows the calls an AI makes to the IDE, not its conversation. The notes are
//! how the context is kept: the AI is told to leave them (`session_note`).

use serde::{Deserialize, Serialize};
use std::{
    fs,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// A session with no call for this long is over.
const IDLE_MS: u64 = 10 * 60 * 1000;
/// The calls kept per session, the oldest dropped first.
const MAX_EVENTS: usize = 1000;
const MAX_NOTE: usize = 4000;
/// The kinds of note an AI may leave.
pub const NOTE_KINDS: [&str; 5] = ["goal", "decision", "progress", "todo", "summary"];

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub at: u64,
    pub tool: String,
    pub summary: String,
    pub ok: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub at: u64,
    pub kind: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    /// The AI that connected, as it names itself, and its version.
    pub client: String,
    pub version: String,
    pub started: u64,
    pub last: u64,
    pub ended: Option<u64>,
    pub title: Option<String>,
    pub events: Vec<Event>,
    pub notes: Vec<Note>,
    #[serde(skip)]
    pub root: PathBuf,
}

impl Session {
    pub fn active(&self, at: u64) -> bool {
        self.ended.is_none() && at.saturating_sub(self.last) < IDLE_MS
    }

    /// The name shown in lists: the goal the AI gave, else who and when.
    pub fn display_title(&self) -> String {
        match &self.title {
            Some(title) => title.clone(),
            None => format!("{} · {}", self.client, clock(self.started)),
        }
    }
}

/// `jj/mm hh:mm` of a time in milliseconds, in UTC (the IDE has no time zone database).
pub fn clock(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rest) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let _ = era;
    format!(
        "{day:02}/{month:02} {:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60
    )
}

fn short(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

#[derive(Default)]
pub struct Store {
    pub sessions: Vec<Session>,
    root: PathBuf,
}

fn folder(root: &Path) -> PathBuf {
    root.join(".atelier").join("sessions")
}

impl Store {
    /// The project the sessions belong to. Its sessions are read from disk when it changes.
    pub fn set_root(&mut self, root: &Path) {
        if self.root == root {
            return;
        }
        self.root = root.to_path_buf();
        self.sessions = fs::read_dir(folder(root))
            .map(|dir| {
                dir.flatten()
                    .filter(|f| f.path().extension().is_some_and(|e| e == "json"))
                    .filter_map(|f| {
                        serde_json::from_slice::<Session>(&fs::read(f.path()).ok()?).ok()
                    })
                    .map(|mut s| {
                        s.root = root.to_path_buf();
                        s
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.sessions.sort_by_key(|s| std::cmp::Reverse(s.started));
    }

    fn save(&self, session: &Session) {
        if self.root.as_os_str().is_empty() {
            return;
        }
        let dir = folder(&self.root);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        // The sessions stay out of version control unless the user decides otherwise.
        crate::atelier::ensure_ignored(&self.root, "sessions/");
        if let Ok(json) = serde_json::to_vec_pretty(session) {
            let _ = fs::write(dir.join(format!("{}.json", session.id)), json);
        }
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.id == id)
    }

    /// A new session for an AI that has just connected. Its id.
    pub fn start(&mut self, client: &str, version: &str) -> String {
        let random = std::hash::RandomState::new().build_hasher().finish() & 0xffff;
        let at = now();
        let id = format!("{at:012x}{random:04x}");
        let session = Session {
            id: id.clone(),
            client: if client.is_empty() {
                "IA".into()
            } else {
                client.to_string()
            },
            version: version.to_string(),
            started: at,
            last: at,
            ended: None,
            title: None,
            events: Vec::new(),
            notes: Vec::new(),
            root: self.root.clone(),
        };
        self.save(&session);
        self.sessions.insert(0, session);
        id
    }

    /// The session a request belongs to: the one it names, else the most recent one still going,
    /// else a new one (a client that does not send the session header).
    pub fn current(&mut self, id: Option<&str>) -> String {
        if let Some(id) = id.filter(|id| self.index(id).is_some()) {
            return id.to_string();
        }
        let at = now();
        match self.sessions.iter().find(|s| s.active(at)) {
            Some(session) => session.id.clone(),
            None => self.start("IA", ""),
        }
    }

    pub fn record(&mut self, id: &str, tool: &str, summary: &str, ok: bool) {
        let Some(at) = self.index(id) else { return };
        let session = &mut self.sessions[at];
        session.last = now();
        session.events.push(Event {
            at: session.last,
            tool: tool.to_string(),
            summary: short(summary, 160),
            ok,
        });
        if session.events.len() > MAX_EVENTS {
            session.events.remove(0);
        }
        let session = session.clone();
        self.save(&session);
    }

    pub fn note(&mut self, id: &str, kind: &str, text: &str) -> Result<(), String> {
        if !NOTE_KINDS.contains(&kind) {
            return Err(trf!(
                "mcp.kind_one_not",
                join = NOTE_KINDS.join(", "),
                kind = kind
            ));
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(tr!("mcp.text_required").into());
        }
        let at = self.index(id).ok_or(tr!("mcp.unknown_session_lower"))?;
        let session = &mut self.sessions[at];
        session.last = now();
        if kind == "goal" && session.title.is_none() {
            session.title = Some(short(text, 80));
        }
        session.notes.push(Note {
            at: session.last,
            kind: kind.to_string(),
            text: text.chars().take(MAX_NOTE).collect(),
        });
        let session = session.clone();
        self.save(&session);
        Ok(())
    }

    /// The client has closed its session.
    pub fn end(&mut self, id: &str) {
        let Some(at) = self.index(id) else { return };
        self.sessions[at].ended = Some(now());
        let session = self.sessions[at].clone();
        self.save(&session);
    }

    pub fn delete(&mut self, id: &str) {
        if let Some(at) = self.index(id) {
            let session = self.sessions.remove(at);
            let _ = fs::remove_file(folder(&self.root).join(format!("{}.json", session.id)));
            let _ = fs::remove_file(folder(&self.root).join(format!("{}.md", session.id)));
        }
    }

    /// What the next AI needs to take the session up: the notes by kind, the files the AI had
    /// the IDE show, and its latest calls.
    pub fn markdown(&self, id: &str) -> Option<String> {
        let session = &self.sessions[self.index(id)?];
        let mut out = trf!("mcp.atelier_session", session = session.display_title());
        out.push_str(&trf!(
            "mcp.identifier_ai_start",
            id = session.id,
            client = session.client,
            version = session.version,
            started = clock(session.started)
        ));
        out.push_str(&trf!("mcp.tool_calls", count = session.events.len()));
        let sections = [
            ("goal", tr!("mcp.goal")),
            ("decision", tr!("mcp.decisions")),
            ("todo", tr!("mcp.do")),
            ("progress", tr!("mcp.progress")),
            ("summary", tr!("mcp.summary")),
        ];
        for (kind, title) in sections {
            let notes: Vec<&Note> = session.notes.iter().filter(|n| n.kind == kind).collect();
            if notes.is_empty() {
                continue;
            }
            out.push_str(&format!("## {title}\n\n"));
            for note in notes {
                out.push_str(&format!("- {}\n", note.text.replace('\n', "\n  ")));
            }
            out.push('\n');
        }
        let mut shown: Vec<&str> = Vec::new();
        for event in session
            .events
            .iter()
            .filter(|e| e.tool == "open_file" && e.ok)
        {
            if !shown.contains(&event.summary.as_str()) {
                shown.push(&event.summary);
            }
        }
        if !shown.is_empty() {
            out.push_str(tr!("mcp.files_ai_showed_ide"));
            for file in shown.iter().take(30) {
                out.push_str(&format!("- {file}\n"));
            }
            out.push('\n');
        }
        let recent: Vec<&Event> = session.events.iter().rev().take(25).collect();
        if !recent.is_empty() {
            out.push_str(tr!("mcp.latest_calls_ide"));
            for event in recent.into_iter().rev() {
                out.push_str(&format!(
                    "- {} `{}` {}{}\n",
                    clock(event.at),
                    event.tool,
                    event.summary,
                    if event.ok { "" } else { tr!("mcp.failed") }
                ));
            }
            out.push('\n');
        }
        out.push_str("---\nCe contexte vient de l'IDE Atelier : il ne contient pas la conversation, seulement les notes laissées par l'IA \
                      et ses appels à l'IDE. Continue le travail à partir de là et garde-le à jour avec l'outil `session_note`.\n");
        Some(out)
    }

    /// Writes the context of a session in the project, for an AI that reads files. Its path
    /// from the project.
    pub fn export(&self, id: &str) -> Result<String, String> {
        let text = self.markdown(id).ok_or(tr!("mcp.unknown_session_lower"))?;
        let dir = folder(&self.root);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{id}.md")), text).map_err(|e| e.to_string())?;
        Ok(format!(".atelier/sessions/{id}.md"))
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/mcp/sessions/tests.rs"]
mod tests;
