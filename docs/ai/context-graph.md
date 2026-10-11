# Context graph

A map of the project, made **without AI** and kept up to date, that agents read instead of opening dozens of files, so they use less context.

## Turning it on

In a repository where you have not decided yet, the Project window offers **Generate the context graph?**; the graph button of its header does the same at any time (generate, open the report, regenerate, turn off). The answer is saved in `.atelier/config.json`.

While it is on, a thread of its own watches the files (on each save and every 10 seconds), reads again only those that changed and rebuilds the graph, as a background task shown in the status bar.

## What is read

- JavaScript and TypeScript;
- Rust: functions, types, methods, imports, modules, calls, inheritance;
- the headings and links of Markdown files.

Each link is `EXTRACTED` (read in the code) or `INFERRED` (deduced from a name that is unique in the project). Files that rely on each other form groups.

## Files

In `.atelier/graph/`:

| File | Content |
| --- | --- |
| `GRAPH_REPORT.md` | The summary to read first: central items, groups, and an estimate of the share of tokens saved. Written in English, for AI agents. |
| `graph.json` | The graph, in node-link format |
| `cache.json` | What each file said, so that nothing is read again after a restart |

The folder is ignored by git (`.atelier/.gitignore`): the graph regenerates, there is nothing to share. Remove the line to commit it.

## For AI agents

Agents query it through the `graph_overview`, `graph_query`, `graph_node` and `graph_path` MCP tools, whose answers stop at a token budget. The `commit` command also uses it to group files (see [Commits by AI](./commits)).
