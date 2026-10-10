use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-sessions-{name}-{}", std::process::id()));
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
