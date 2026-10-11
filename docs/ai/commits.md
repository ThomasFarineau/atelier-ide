# Commits by AI

The MCP server offers commands (MCP prompts) for commits that follow the conventions of the repository. In Claude Code they are `/mcp__atelier__commit`, `/mcp__atelier__rework-commit`…

## `commit`

Two modes, also available on their own as `commit-all` and `commit-by-context`:

- **all**: a single commit with every change;
- **context**: one commit per coherent change. Files that rely on each other (through the [context graph](./context-graph), when it is on) or that share a folder go together; docs, dependencies (`Cargo.lock`, `package.json`…) and CI each get their own commit. Eight commits at most.

The agent goes through three tools: `git_commit_plan` (the conventions of the repository, read from its history, the files and how to split them), `git_diff` (what changed) and `git_commit` (the commits).

The **conventions of the repository** are deduced from its last 60 commits: Conventional Commits, gitmoji or free text, French or English, the types and scopes used, capital letter, final period, length, presence of a body. Every message is checked before any commit: if a single one does not follow them, nothing is committed.

**Never a co-author**: `Co-authored-by` lines and attributions to an AI are removed from the message, whoever wrote it. Nothing is pushed or changed afterwards (no amend).

## `rework-commit`

`/rework-commit last 10 commits` (also “since abc123” or `abc123..HEAD`) reviews the last commits and fixes the messages that do not fit: does the **title** follow the conventions of the repository and say what the diff really does, and does the **description** match the changes, without claiming anything false?

`git_rework_plan` lists the commits (subject, body, changed files) with automatic findings:

- title outside the conventions;
- a type that contradicts the files (`docs:` on code);
- a scope missing from every changed path;
- files named in the message but not changed by the commit;
- a co-author;
- a big change without a description.

The agent reads the diffs (`git_show`) and judges; `git_rework_apply` rewrites only the chosen messages. Trees, authors and dates are kept as they are: no file moves and no conflict is possible.

Safeguards:

- no merge in the chain, and a clean working tree;
- every message checked before anything is touched;
- **commits already pushed are refused**, unless the user explicitly agrees (`allowPushed`);
- the old head stays under `refs/atelier/backup/<date>` (`git reset --hard <ref>` puts everything back);
- never a co-author, never a push.
