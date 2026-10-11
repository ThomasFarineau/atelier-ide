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
    let path =
        json!({"type": "string", "description": "Absolute path, or relative to the project."});
    vec![
        json!({
            "name": "get_project_info",
            "description": "The project open in the IDE: folder, active file, number of tabs and of problems.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "get_open_files",
            "description": "The tabs open in the editor, with the active file and those that have unsaved changes.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "get_active_editor",
            "description": "The active editor: file, language, caret position (lines and columns from 1) and selected text.",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        json!({
            "name": "open_file",
            "description": "Opens a file in the editor of the IDE, optionally at a line, for the user to see it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "line": {"type": "integer", "minimum": 1, "description": "Line to show, from 1."},
                    "column": {"type": "integer", "minimum": 1, "description": "Column, from 1."},
                },
                "required": ["path"],
            },
        }),
        json!({
            "name": "get_problems",
            "description": "The errors and warnings the IDE found in the open files (live language analysis), with their position.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Only this file."},
                    "severity": {"type": "string", "enum": ["error", "warning"], "description": "Only this level."},
                },
            },
        }),
        json!({
            "name": "session_note",
            "description": "Keeps context in the Atelier session of the project, so that it can be taken up later, by you or by another AI. Note the goal at the start (goal), then decisions, remaining tasks and progress.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["goal", "decision", "progress", "todo", "summary"], "description": "goal: the goal · decision: a choice made · todo: what is left · progress: what is done · summary: final summary."},
                    "text": {"type": "string"},
                },
                "required": ["kind", "text"],
            },
        }),
        json!({
            "name": "get_session_context",
            "description": "The context of an Atelier session (goal, decisions, tasks, files shown, latest calls) to take it up. Without “id”, the one of this connection.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string", "description": "Identifier of a session of the project (see list_sessions)."}}},
        }),
        json!({
            "name": "list_sessions",
            "description": "The AI sessions saved in the project, most recent first.",
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
            "description": "Searches text in the files of the project (like “Find in Files” of the IDE, without ignored folders or binaries).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Text or regular expression."},
                    "regex": {"type": "boolean", "description": "The query is a regular expression."},
                    "caseSensitive": {"type": "boolean"},
                    "wholeWord": {"type": "boolean"},
                    "fileMask": {"type": "string", "description": "File mask, e.g. “*.rs, src/*”."},
                    "maxResults": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS, "description": "50 by default."},
                },
                "required": ["query"],
            },
        }),
        crate::plugins::builtins::translations::mcp_spec(),
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
            None => Err("No file is open in the editor".into()),
        },
        "open_file" => {
            let path = resolve(&root, text(args, "path").ok_or("“path” is required")?);
            if !path.is_file() {
                return Err(format!("File not found: {}", path.display()));
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
                    return Err(format!("“severity” is error or warning, not {other}"));
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
        other => Err(format!("unknown tool: {other}")),
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
    let query = text(args, "query").ok_or("“query” is required")?;
    let flag = |key: &str| args.get(key).and_then(Value::as_bool).unwrap_or(false);
    let options = crate::search::Options {
        case: flag("caseSensitive"),
        word: flag("wholeWord"),
        regex: flag("regex"),
    };
    let re =
        crate::search::build(query, options).map_err(|e| format!("Invalid expression: {e}"))?;
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
#[path = "../../../../test/plugins/builtins/mcp/tools/tests.rs"]
mod tests;
