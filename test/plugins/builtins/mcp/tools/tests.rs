use super::*;
use crate::plugins::builtin::{EditorSnapshot, OpenDocument, Problem};

struct Fake {
    root: PathBuf,
    opened: Option<(PathBuf, usize, usize)>,
}

impl Host for Fake {
    fn root(&self) -> &Path {
        &self.root
    }
    fn active_file(&self) -> Option<PathBuf> {
        Some(self.root.join("a.js"))
    }
    fn open_file(&mut self, _: PathBuf) {}
    fn open_at(&mut self, path: PathBuf, line: usize, column: usize, _: usize) {
        self.opened = Some((path, line, column));
    }
    fn open_project(&mut self, _: PathBuf) {}
    fn save_files(&mut self) -> bool {
        true
    }
    fn reload_files(&mut self) {}
    fn show_tool_window(&mut self, _: &'static crate::plugins::builtin::ToolWindow) {}
    fn notify(&mut self, _: String) {}
    fn problems(&self) -> Vec<Problem> {
        let problem = |name: &str, severity, message: &str| Problem {
            path: self.root.join(name),
            line: 4,
            column: 2,
            length: 3,
            severity,
            message: message.into(),
            source: "syntaxe".into(),
        };
        vec![
            problem("a.js", 1, "Syntaxe inattendue"),
            problem("b.js", 2, "Variable inutilisée"),
        ]
    }
    fn open_documents(&self) -> Vec<OpenDocument> {
        vec![OpenDocument {
            path: Some(self.root.join("a.js")),
            title: "a.js".into(),
            dirty: true,
            active: true,
        }]
    }
    fn editor_state(&self) -> Option<EditorSnapshot> {
        Some(EditorSnapshot {
            path: Some(self.root.join("a.js")),
            language: "js".into(),
            line: 5,
            column: 3,
            selection: Some("let x".into()),
        })
    }
}

fn fake() -> Fake {
    // One folder per test: they run side by side and each removes its own at the end.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("atelier-mcp-tools-{}-{n}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("a.js"), "let x = 1;\n").unwrap();
    Fake { root, opened: None }
}

fn json_of(result: Result<String, String>) -> Value {
    serde_json::from_str(&result.expect("the tool answered")).unwrap()
}

#[test]
fn every_tool_has_a_schema_and_the_editor_tools_answer() {
    for spec in specs() {
        assert!(
            spec["name"].is_string()
                && spec["description"].is_string()
                && spec["inputSchema"]["type"] == "object",
            "{spec}"
        );
    }
    let mut host = fake();
    let info = json_of(run(&mut host, "get_project_info", &json!({})));
    assert_eq!(
        (
            info["errors"].as_u64(),
            info["warnings"].as_u64(),
            info["openFiles"].as_u64()
        ),
        (Some(1), Some(1), Some(1))
    );
    let files = json_of(run(&mut host, "get_open_files", &json!({})));
    assert_eq!(files[0]["unsavedChanges"], true);
    let editor = json_of(run(&mut host, "get_active_editor", &json!({})));
    assert_eq!(
        (editor["line"].as_u64(), editor["selectedText"].as_str()),
        (Some(5), Some("let x"))
    );
    let _ = std::fs::remove_dir_all(&host.root);
}

#[test]
fn problems_are_filtered_by_file_and_level_and_counted_from_one() {
    let mut host = fake();
    let all = json_of(run(&mut host, "get_problems", &json!({})));
    assert_eq!(all["count"], 2);
    assert_eq!(
        (
            all["problems"][0]["line"].as_u64(),
            all["problems"][0]["column"].as_u64()
        ),
        (Some(5), Some(3))
    );
    let errors = json_of(run(
        &mut host,
        "get_problems",
        &json!({"severity": "error"}),
    ));
    assert_eq!(errors["count"], 1);
    let one_file = json_of(run(&mut host, "get_problems", &json!({"path": "b.js"})));
    assert_eq!(one_file["problems"][0]["severity"], "warning");
    assert!(run(&mut host, "get_problems", &json!({"severity": "grave"})).is_err());
    let _ = std::fs::remove_dir_all(&host.root);
}

#[test]
fn a_file_opens_at_its_line_and_a_missing_one_is_refused() {
    let mut host = fake();
    run(
        &mut host,
        "open_file",
        &json!({"path": "a.js", "line": 12, "column": 4}),
    )
    .unwrap();
    assert_eq!(
        host.opened,
        Some((host.root.join("a.js"), 11, 3)),
        "0-based inside the IDE"
    );
    assert!(run(&mut host, "open_file", &json!({"path": "nope.js"})).is_err());
    assert!(run(&mut host, "open_file", &json!({})).is_err());
    assert!(run(&mut host, "inconnu", &json!({})).is_err());
    let _ = std::fs::remove_dir_all(&host.root);
}

#[test]
fn the_search_reports_files_lines_and_columns_from_one() {
    let host = fake();
    std::fs::write(
        host.root.join("src").join("lib.rs"),
        "fn main() {}\n// TODO: later\n",
    )
    .unwrap();
    std::fs::write(host.root.join("a.js"), "// todo here\n").unwrap();
    let found = json_of(search(&host.root, &json!({"query": "todo"})));
    assert_eq!(found["matches"], 2);
    let only_rust = json_of(search(
        &host.root,
        &json!({"query": "TODO", "caseSensitive": true, "fileMask": "*.rs"}),
    ));
    assert_eq!(only_rust["matches"], 1);
    let hit = &only_rust["files"][0];
    assert_eq!(
        (hit["path"].as_str(), hit["matches"][0]["line"].as_u64()),
        (Some("src/lib.rs"), Some(2))
    );
    assert!(
        search(&host.root, &json!({"query": "("  , "regex": true})).is_err(),
        "an invalid expression is reported"
    );
    assert!(search(&host.root, &json!({})).is_err());
    let _ = std::fs::remove_dir_all(&host.root);
}
