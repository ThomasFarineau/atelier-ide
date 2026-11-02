---
name: Git
category: vcs
icon: icons/git.svg
summary: Commit, branches, history, diff, push/pull, stash, conflicts and annotations, with the
  installed git.
tags: [blame, merge, rebase, branch, commit, vcs]
---

# Git

The built-in Git plugin uses the installed `git` command (2.26 or newer).

## Commit window

**Alt+0**, on the left rail: changes, conflicts and unversioned files, with checkboxes. Only the checked files are committed, without going through the index.

- **Amend** the last commit and **Sign-off** options, **Commit** and **Commit and Push…**.
- Context menu: diff, open, history, rollback, add to Git or to `.gitignore`, copy the path.
- **Stash** tab: stash, apply, pop, drop, and see the files of a stash.

## Git window

**Alt+9**, on the bottom rail: the log with the branch graph, branch and tag badges, filters by text, hash, author or branch, and the history of a file. The details of a commit show its message and its files.

Actions: checkout, new branch, tag, cherry-pick, revert, reset (soft, mixed, hard, keep) and undo the last commit. The **Console** tab lists the git commands the IDE ran and their output.

## Branch widget

In the title bar: the current branch, the commits ahead and behind, the operation in progress. It gives access to:

- local and remote branches, smart checkout (stash, checkout, then restore), create, rename and delete branches, merge and rebase;
- update (pull with merge or rebase, with autostash), push (outgoing commits, force-with-lease, tags), fetch;
- checkout of a tag or a revision, and the remotes.

## Diff and conflicts

- **Diff** side by side or unified, with navigation between differences (**F7** / **Shift+F7**) and hiding of unchanged lines.
- **Conflicts**: accept yours or theirs, mark as resolved, or merge chunk by chunk in the resolution window. A merge, rebase or cherry-pick in progress can be aborted or continued.

## In the editor and the tree

Markers for the lines added, changed or deleted since HEAD, and **Git Blame** annotations, from a right click on the line numbers. The project tree is colored by Git status.

## More

- Create a repository or clone one, from the widget or Search Everywhere.
- Operations run in the background, and the status refreshes every 5 seconds or when you come back to the window.
- Open files are saved before a commit, a checkout or an update, then reloaded if they changed on disk.
- The plugin is automatic (`"git": "auto"` in `.atelier/config.json`): on while the project has a `.git` (a folder, or the file of a worktree or a submodule), so it comes on by itself after a `git init`, and off without one. Turning it on or off in the Plugins window decides for the project.

| Shortcut | Action |
| --- | --- |
| Ctrl+K | Commit |
| Ctrl+Shift+K | Push |
| Ctrl+T | Update project (pull) |
| Alt+0 / Alt+9 | Commit / Git windows |
| F7 / Shift+F7 | Next / previous difference |

## GitHub

**GitHub** is a sub-plugin of Git: it runs only while Git is on, and can be turned off by itself in the **Plugins** window. It shows in projects whose repository has a GitHub remote, or a `.github` folder.

- The **Pull Request** window, on the right rail, shows the pull request of the current branch: its state, the decision of the reviews, each review with its comment, and the comments of the reviews on the code, by conversation. A conversation opens its file at its line; one whose code changed since is marked *outdated*. **Refresh** reads it again, **Open on GitHub** opens it in the browser.
- It goes through the GitHub CLI: install [`gh`](https://cli.github.com) and sign in with `gh auth login`.
- It brings the [schemas](../languages/json#schemas) of the workflows of GitHub Actions (`.github/workflows/*.yml`) and of `.github/dependabot.yml`: their values are checked, their keys completed and described under the mouse.
- Search Everywhere: **GitHub: Show Pull Request**.
