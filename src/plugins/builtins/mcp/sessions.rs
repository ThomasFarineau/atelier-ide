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
            return Err(format!(
                "« kind » vaut {}, pas {kind}",
                NOTE_KINDS.join(", ")
            ));
        }
        let text = text.trim();
        if text.is_empty() {
            return Err("« text » est obligatoire".into());
        }
        let at = self.index(id).ok_or("session inconnue")?;
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
        let mut out = format!("# Session Atelier : {}\n\n", session.display_title());
        out.push_str(&format!(
            "- Identifiant : `{}`\n- IA : {} {}\n- Début : {}\n",
            session.id,
            session.client,
            session.version,
            clock(session.started)
        ));
        out.push_str(&format!("- Appels d'outils : {}\n\n", session.events.len()));
        let sections = [
            ("goal", "Objectif"),
            ("decision", "Décisions"),
            ("todo", "À faire"),
            ("progress", "Avancement"),
            ("summary", "Résumé"),
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
            out.push_str("## Fichiers que l'IA a montrés dans l'IDE\n\n");
            for file in shown.iter().take(30) {
                out.push_str(&format!("- {file}\n"));
            }
            out.push('\n');
        }
        let recent: Vec<&Event> = session.events.iter().rev().take(25).collect();
        if !recent.is_empty() {
            out.push_str("## Derniers appels à l'IDE\n\n");
            for event in recent.into_iter().rev() {
                out.push_str(&format!(
                    "- {} `{}` {}{}\n",
                    clock(event.at),
                    event.tool,
                    event.summary,
                    if event.ok { "" } else { " (échec)" }
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
        let text = self.markdown(id).ok_or("session inconnue")?;
        let dir = folder(&self.root);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{id}.md")), text).map_err(|e| e.to_string())?;
        Ok(format!(".atelier/sessions/{id}.md"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("atelier-sessions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_session_keeps_its_calls_and_notes_in_the_project_and_comes_back_from_disk() {
        let root = scratch("persist");
        let mut store = Store::default();
        store.set_root(&root);
        let id = store.start("claude-code", "2.1");
        store.record(&id, "open_file", "src/main.rs:12", true);
        store.record(&id, "get_problems", "{}", false);
        store
            .note(&id, "goal", "Corriger le plantage au démarrage")
            .unwrap();
        store
            .note(&id, "decision", "On garde l'API actuelle")
            .unwrap();
        assert!(
            root.join(".atelier/sessions")
                .join(format!("{id}.json"))
                .exists()
        );
        assert_eq!(
            fs::read_to_string(root.join(".atelier/.gitignore")).unwrap(),
            "sessions/\n"
        );
        // Another run of the IDE, or another window: the same project shows the same sessions.
        let mut again = Store::default();
        again.set_root(&root);
        assert_eq!(again.sessions.len(), 1);
        let session = &again.sessions[0];
        assert_eq!(
            (
                session.client.as_str(),
                session.events.len(),
                session.notes.len()
            ),
            ("claude-code", 2, 2)
        );
        assert_eq!(session.display_title(), "Corriger le plantage au démarrage");
        assert_eq!(session.root, root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_request_without_a_session_goes_to_the_one_that_is_going_or_starts_one() {
        let root = scratch("current");
        let mut store = Store::default();
        store.set_root(&root);
        let first = store.current(None);
        assert_eq!(
            store.sessions[0].client, "IA",
            "an AI that never said who it is"
        );
        assert_eq!(store.current(None), first, "the session still going");
        assert_eq!(
            store.current(Some("inconnue")),
            first,
            "an id that is not known falls back the same way"
        );
        store.end(&first);
        assert_ne!(
            store.current(None),
            first,
            "a session that ended is not taken up again"
        );
        assert_eq!(store.sessions.len(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn notes_are_checked_and_the_context_is_written_for_the_next_ai() {
        let root = scratch("context");
        let mut store = Store::default();
        store.set_root(&root);
        let id = store.start("codex", "0.9");
        assert!(store.note(&id, "blague", "x").is_err(), "unknown kind");
        assert!(store.note(&id, "todo", "  ").is_err(), "empty note");
        store.note(&id, "goal", "Ajouter le plugin MCP").unwrap();
        store.note(&id, "todo", "Écrire les tests").unwrap();
        store.note(&id, "progress", "Le serveur répond").unwrap();
        store.record(&id, "open_file", "src/server.rs:1", true);
        store.record(&id, "open_file", "src/server.rs:1", true);
        let text = store.markdown(&id).unwrap();
        assert!(
            text.contains("## Objectif") && text.contains("Ajouter le plugin MCP"),
            "{text}"
        );
        assert!(text.contains("## À faire") && text.contains("Écrire les tests"));
        assert_eq!(
            text.matches("- src/server.rs:1").count(),
            1,
            "a file shown twice is listed once"
        );
        assert!(
            text.contains("session_note"),
            "the next AI is told how to go on"
        );
        let path = store.export(&id).unwrap();
        assert_eq!(path, format!(".atelier/sessions/{id}.md"));
        assert!(root.join(&path).exists());
        store.delete(&id);
        assert!(
            store.sessions.is_empty() && !root.join(&path).exists(),
            "the session and its context are gone"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn times_are_shown_as_day_month_hour_minute() {
        // 2026-10-09 14:32:10 UTC
        assert_eq!(clock(1_791_556_330_000), "09/10 14:32");
        assert_eq!(clock(0), "01/01 00:00");
    }
}
