# MCP server & sessions

The **Atelier Super Intelligence Tools** plugin makes Atelier an MCP (Model Context Protocol) server, so that an AI agent started in one of its terminals can use the IDE: HTTP on `127.0.0.1` only, behind a secret kept from one session to the next.

## Connecting an agent

The terminals of the IDE define `ATELIER_MCP_URL` and `ATELIER_MCP_TOKEN`. When you start `claude`, `gemini` or `codex` in one of them, the IDE holds the Enter key and offers to connect the agent. **Connect** writes the server:

| Agent | File |
| --- | --- |
| Claude Code | `.mcp.json`, and the approval in `.claude/settings.local.json` |
| Gemini CLI | `.gemini/settings.json` |
| Codex | `.codex/config.toml` |

No file holds the secret (`${ATELIER_MCP_TOKEN}` or `bearer_token_env_var`). Claude and Gemini also read the address from `${ATELIER_MCP_URL}`, so their files stay valid when the port changes. Codex does not expand variables: its file holds the current address and is rewritten when it changes (the port is kept between sessions, which avoids it most of the time), so better not to commit it. The answer is remembered for each project, and the **MCP: Connect…** commands of Search Everywhere do the same on demand.

Another MCP client can connect with the address and the secret (`mcp.json` in the data folder of Atelier): `Authorization: Bearer <secret>`.

## Tools

| Tool | Gives |
| --- | --- |
| `get_project_info` | The project |
| `get_open_files` | The open files |
| `get_active_editor` | The active editor, caret and selection |
| `open_file` | Opens a file, at a line |
| `get_problems` | Errors and warnings of the open files |
| `search_in_project` | Searches the project, off the interface thread (the window does not freeze) |
| `i18n` | [Translations](../tools/translations) |
| `graph_overview`, `graph_query`, `graph_node`, `graph_path` | The [context graph](./context-graph) |
| `git_commit_plan`, `git_diff`, `git_commit`, `git_rework_plan`, `git_show`, `git_rework_apply` | [Commits by AI](./commits) |
| `session_note`, `get_session_context`, `list_sessions` | AI sessions, below |

## AI sessions

Each connection of an agent to the server is a **session of the project**, saved in `.atelier/sessions/` (ignored by git through `.atelier/.gitignore`). The **AI Sessions** window (**Alt+7**) lists them, with the agent, the time, the calls made to the IDE and the notes left.

The protocol does not show the conversation, only the tool calls. So the IDE keeps the context: the agent is asked (in the server instructions) to note the goal, the decisions, the remaining tasks and the progress with `session_note`.

- **Resume with…** writes this context to `.atelier/sessions/<id>.md`, connects the chosen agent (Claude Code, Codex or Gemini CLI, not necessarily the one that started) and starts it in a new terminal, with a first request that points to that file. The agent can also call `get_session_context` (the `id` of a session) and `list_sessions`.
- **Copy Context** puts it on the clipboard, for any other tool.

## Installed agents

The **Installed AIs** tab of the window shows the state of Claude Code, Codex and Gemini CLI:

- installed or not (the program found on the PATH, its version, otherwise a button that installs it with npm);
- connected or not to the IDE's MCP server for this project (with a button to connect it);
- its usage, read from the files it keeps on the computer (`~/.claude/projects`, `~/.codex/sessions`, `~/.gemini/tmp`): tokens over 24 hours, 7 and 30 days (input / cache / output on hover), the share of this project, sessions and answers, the models used, the last use, the sessions and calls made through Atelier and, for Codex, the usage limits of the subscription (5-hour window and week, with their reset).

Nothing is asked to the servers of the agents: only the usage of this computer is counted. The files are read again only when they changed, in the background, at most once a minute while the tab is shown.
