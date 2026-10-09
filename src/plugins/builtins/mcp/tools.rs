//! The tools the IDE offers to an AI through MCP. Each one has a name, a description and a JSON
//! schema of its arguments ([`specs`]), and is run against the IDE by [`run`] on the interface
//! thread, except the project search ([`search`]), which runs off it.

use crate::plugins::builtin::Host;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

/// The answers of the graph tools stop at this many tokens unless asked otherwise (about four
/// characters each).
const DEFAULT_TOKENS: usize = 1500;
const DEFAULT_RESULTS: usize = 50;
const MAX_RESULTS: usize = 500;

/// The tools, as the `tools/list` answer describes them.
pub fn specs() -> Vec<Value> {
    let path = json!({"type": "string", "description": "Chemin absolu, ou relatif au projet."});
    vec![
        json!({
            "name": "get_project_info",
            "description": "Le projet ouvert dans l'IDE : dossier, fichier actif, nombre d'onglets et de problèmes.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "get_open_files",
            "description": "Les onglets ouverts dans l'éditeur, avec le fichier actif et ceux qui ont des modifications non enregistrées.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "get_active_editor",
            "description": "L'éditeur actif : fichier, langage, position du curseur (lignes et colonnes à partir de 1) et texte sélectionné.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "open_file",
            "description": "Ouvre un fichier dans l'éditeur de l'IDE, éventuellement à une ligne, pour que l'utilisateur le voie.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "line": {"type": "integer", "minimum": 1, "description": "Ligne à montrer, à partir de 1."},
                    "column": {"type": "integer", "minimum": 1, "description": "Colonne, à partir de 1."},
                },
                "required": ["path"],
            },
        }),
        json!({
            "name": "get_problems",
            "description": "Les erreurs et avertissements que l'IDE a trouvés dans les fichiers ouverts (analyse du langage en direct), avec leur position.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Ne garder que ce fichier."},
                    "severity": {"type": "string", "enum": ["error", "warning"], "description": "Ne garder que ce niveau."},
                },
            },
        }),
        json!({
            "name": "session_note",
            "description": "Garde du contexte dans la session Atelier du projet, pour qu'elle puisse être reprise plus tard, par toi ou par une autre IA. Note l'objectif au début (goal), puis décisions, tâches restantes et avancement.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["goal", "decision", "progress", "todo", "summary"], "description": "goal : l'objectif · decision : un choix fait · todo : reste à faire · progress : ce qui est fait · summary : résumé de fin."},
                    "text": {"type": "string"},
                },
                "required": ["kind", "text"],
            },
        }),
        json!({
            "name": "get_session_context",
            "description": "Le contexte d'une session d'Atelier (objectif, décisions, tâches, fichiers montrés, derniers appels) pour la reprendre. Sans « id », celle de cette connexion.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "description": "Identifiant d'une session du projet (voir list_sessions)."}}},
        }),
        json!({
            "name": "list_sessions",
            "description": "Les sessions d'IA enregistrées dans le projet, les plus récentes d'abord.",
            "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 100}}},
        }),
        json!({
            "name": "graph_overview",
            "description": "The project's context graph as a summary: central elements and file groups, with an estimate of the tokens it saves. Read it before opening many files: it says where things are. Ask the user to turn it on in Atelier if it is off.",
            "inputSchema": {"type": "object", "properties": {"budget": {"type": "integer", "minimum": 200, "description": "Maximum size of the answer, in tokens (1500 by default)."}}},
        }),
        json!({
            "name": "graph_query",
            "description": "Looks in the context graph for the functions, classes, files… that match a few words, with what they are linked to (calls, imports, inheritance). Much cheaper than reading the files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "A few words of the code, or a short question."},
                    "budget": {"type": "integer", "minimum": 200, "description": "Maximum size of the answer, in tokens (1500 by default)."},
                },
                "required": ["question"],
            },
        }),
        json!({
            "name": "graph_node",
            "description": "Everything the graph knows about a name (function, class, file): where it is defined, what it calls, who calls it, what it imports, its group.",
            "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]},
        }),
        json!({
            "name": "graph_path",
            "description": "The shortest chain of links between two names of the graph (for example from a UI function to the code it runs).",
            "inputSchema": {"type": "object", "properties": {"from": {"type": "string"}, "to": {"type": "string"}}, "required": ["from", "to"]},
        }),
        json!({
            "name": "git_commit_plan",
            "description": "First step of a commit. Returns the repository's commit conventions (read from its history: style, language, types, scopes), the changed files, and in “context” mode how to split them into coherent commits. Write the messages from it, then call git_commit.",
            "inputSchema": {
                "type": "object",
                "properties": {"mode": {"type": "string", "enum": ["all", "context"], "description": "all: one commit with everything · context: one commit per coherent change."}},
                "required": ["mode"],
            },
        }),
        json!({
            "name": "git_diff",
            "description": "What changed in the working tree (diff against HEAD, new files included), to write commit messages from.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "files": {"type": "array", "items": {"type": "string"}, "description": "Only these paths, from the repository root."},
                    "maxChars": {"type": "integer", "minimum": 1000, "maximum": 100000},
                },
            },
        }),
        json!({
            "name": "git_commit",
            "description": "Makes the commits. Every message is checked against the repository's conventions first and nothing is committed if one does not follow them. Co-authored-by lines and AI attributions are always removed. Never pushes or amends.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["all", "context"]},
                    "commits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "message": {"type": "string", "description": "Subject, then optionally a blank line and a body."},
                                "files": {"type": "array", "items": {"type": "string"}, "description": "Paths of this commit (required in “context” mode, from git_commit_plan)."},
                            },
                            "required": ["message"],
                        },
                    },
                },
                "required": ["mode", "commits"],
            },
        }),
        json!({
            "name": "git_rework_plan",
            "description": "First step of reworking commit messages. Lists the commits of a range (oldest first) with subject, body and the files each one changes, the repository's commit conventions, and automatic findings (subject against the conventions, type or scope contradicting the files, files named in the message that the commit does not change, co-author lines). Findings are hints: judge each commit against its diff.",
            "inputSchema": {"type": "object", "properties": {"range": {"type": "string", "description": "“last 10 commits”, “10”, “since <commit>” or “<commit>..HEAD”. It must end at HEAD."}}, "required": ["range"]},
        }),
        json!({
            "name": "git_show",
            "description": "One commit with its stat and patch, to judge its message against what it really changes.",
            "inputSchema": {"type": "object", "properties": {"rev": {"type": "string"}, "maxChars": {"type": "integer", "minimum": 1000, "maximum": 100000}}, "required": ["rev"]},
        }),
        json!({
            "name": "git_rework_apply",
            "description": "Rewrites the messages of the chosen commits and nothing else (files, authors and dates stay). Refuses merges, uncommitted changes, messages that break the conventions, and commits already on a remote unless allowPushed is set, which you do only when the user agreed. The previous head is kept under refs/atelier/backup/. Co-authored-by lines and AI attributions are always removed. Never pushes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "commits": {"type": "array", "items": {"type": "object", "properties": {"hash": {"type": "string"}, "message": {"type": "string", "description": "The new message: subject, then optionally a blank line and a body."}}, "required": ["hash", "message"]}},
                    "allowPushed": {"type": "boolean", "description": "Rewrite commits that are already on a remote (needs a force push afterwards). Only with the user's agreement."},
                },
                "required": ["commits"],
            },
        }),
        json!({
            "name": "search_in_project",
            "description": "Cherche du texte dans les fichiers du projet (comme « Rechercher dans les fichiers » de l'IDE, sans dossiers ignorés ni binaires).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Texte ou expression régulière."},
                    "regex": {"type": "boolean", "description": "La requête est une expression régulière."},
                    "caseSensitive": {"type": "boolean"},
                    "wholeWord": {"type": "boolean"},
                    "fileMask": {"type": "string", "description": "Masque de fichiers, ex. « *.rs, src/* »."},
                    "maxResults": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS, "description": "50 par défaut."},
                },
                "required": ["query"],
            },
        }),
    ]
}

fn pretty(value: Value) -> Result<String, String> {
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn number(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

/// A path of the arguments, absolute: relative ones are from the project.
fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let (a, b) = (crate::project::simplify(a), crate::project::simplify(b));
    a == b
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

/// Runs a tool that needs the editor.
pub fn run(host: &mut dyn Host, name: &str, args: &Value) -> Result<String, String> {
    let root = host.root().to_path_buf();
    match name {
        "get_project_info" => {
            let problems = host.problems();
            pretty(json!({
                "name": root.file_name().map(|n| n.to_string_lossy().into_owned()),
                "root": crate::project::simplify(&root),
                "activeFile": host.active_file().map(|p| crate::project::simplify(&p)),
                "openFiles": host.open_documents().len(),
                "errors": problems.iter().filter(|p| p.severity == 1).count(),
                "warnings": problems.iter().filter(|p| p.severity == 2).count(),
            }))
        }
        "get_open_files" => pretty(json!(
            host.open_documents()
                .into_iter()
                .map(|d| json!({
                    "path": d.path.map(|p| crate::project::simplify(&p)),
                    "title": d.title,
                    "unsavedChanges": d.dirty,
                    "active": d.active,
                }))
                .collect::<Vec<_>>()
        )),
        "get_active_editor" => match host.editor_state() {
            Some(editor) => pretty(json!({
                "path": editor.path.map(|p| crate::project::simplify(&p)),
                "language": editor.language,
                "line": editor.line,
                "column": editor.column,
                "selectedText": editor.selection,
            })),
            None => Err("Aucun fichier n'est ouvert dans l'éditeur".into()),
        },
        "open_file" => {
            let path = resolve(&root, text(args, "path").ok_or("« path » est obligatoire")?);
            if !path.is_file() {
                return Err(format!("Fichier introuvable : {}", path.display()));
            }
            let line = number(args, "line").unwrap_or(1).max(1) - 1;
            let column = number(args, "column").unwrap_or(1).max(1) - 1;
            host.open_at(path.clone(), line, column, 0);
            Ok(format!(
                "Ouvert : {}:{}",
                crate::project::simplify(&path).display(),
                line + 1
            ))
        }
        "get_problems" => {
            let only = text(args, "path").map(|p| resolve(&root, p));
            let severity = match text(args, "severity") {
                Some("error") => Some(1),
                Some("warning") => Some(2),
                Some(other) => {
                    return Err(format!("« severity » vaut error ou warning, pas {other}"));
                }
                None => None,
            };
            let problems: Vec<Value> = host
                .problems()
                .into_iter()
                .filter(|p| only.as_ref().is_none_or(|f| same_file(&p.path, f)))
                .filter(|p| severity.is_none_or(|s| p.severity == s))
                .map(|p| {
                    json!({
                        "path": crate::project::simplify(&p.path),
                        "line": p.line + 1,
                        "column": p.column + 1,
                        "severity": match p.severity { 1 => "error", 2 => "warning", 3 => "information", _ => "hint" },
                        "message": p.message,
                        "source": p.source,
                    })
                })
                .collect();
            pretty(json!({"count": problems.len(), "problems": problems}))
        }
        other => Err(format!("outil inconnu : {other}")),
    }
}

/// The tools of the context graph, run off the interface thread. None for another tool.
pub fn graph(name: &str, args: &Value) -> Option<Result<String, String>> {
    if !matches!(
        name,
        "graph_overview" | "graph_query" | "graph_node" | "graph_path"
    ) {
        return None;
    }
    let Some(graph) = crate::graph::current() else {
        return Some(Err("The context graph is not turned on for this project. In Atelier: Project window, “Context graph” button.".into()));
    };
    let budget = number(args, "budget").unwrap_or(DEFAULT_TOKENS).max(200) * 4;
    let argument = |key: &str| text(args, key).ok_or_else(|| format!("“{key}” is required"));
    Some(match name {
        "graph_overview" => {
            let report = crate::graph::report::markdown(&graph);
            // Cut at a line, and say so.
            if report.len() <= budget {
                Ok(report)
            } else {
                let mut cut = budget;
                while !report.is_char_boundary(cut) {
                    cut -= 1;
                }
                let kept = report[..cut]
                    .rsplit_once('\n')
                    .map_or(&report[..cut], |(head, _)| head);
                Ok(format!(
                    "{kept}\n\n… summary cut: raise “budget” or use graph_query."
                ))
            }
        }
        "graph_query" => {
            argument("question").map(|q| crate::graph::query::query(&graph, q, budget))
        }
        "graph_node" => argument("name").map(|n| crate::graph::query::node(&graph, n, budget)),
        _ => argument("from")
            .and_then(|from| argument("to").map(|to| crate::graph::query::path(&graph, from, to))),
    })
}

/// The project search, run off the interface thread.
pub fn search(root: &Path, args: &Value) -> Result<String, String> {
    let query = text(args, "query").ok_or("« query » est obligatoire")?;
    let flag = |key: &str| args.get(key).and_then(Value::as_bool).unwrap_or(false);
    let options = crate::search::Options {
        case: flag("caseSensitive"),
        word: flag("wholeWord"),
        regex: flag("regex"),
    };
    let re =
        crate::search::build(query, options).map_err(|e| format!("Expression invalide : {e}"))?;
    let limit = number(args, "maxResults")
        .unwrap_or(DEFAULT_RESULTS)
        .clamp(1, MAX_RESULTS);
    let mut files = Vec::new();
    let looked = crate::search::search_project(
        root,
        &re,
        text(args, "fileMask").unwrap_or(""),
        &AtomicBool::new(false),
        limit,
        |found| {
            let relative = found
                .path
                .strip_prefix(root)
                .unwrap_or(&found.path)
                .to_path_buf();
            files.push(json!({
            "path": relative.to_string_lossy().replace('\\', "/"),
            "matches": found.hits.iter().map(|h| json!({"line": h.line + 1, "column": h.column + 1, "text": h.text})).collect::<Vec<_>>(),
        }));
        },
    );
    let total: usize = files
        .iter()
        .map(|f| f["matches"].as_array().map_or(0, Vec::len))
        .sum();
    pretty(json!({
        "matches": total,
        "truncated": total >= limit,
        "filesSearched": looked,
        "files": files,
    }))
}

#[cfg(test)]
mod tests {
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
        let root =
            std::env::temp_dir().join(format!("atelier-mcp-tools-{}-{n}", std::process::id()));
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
}
