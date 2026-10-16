# Interface

Atelier has a clean, modern interface: a title bar with the project, branch and run widgets, tool window rails, rounded islands, tabs, Search Everywhere, notifications and a status bar with breadcrumbs.

## Look and feel

- Crisp icons, the Source Sans 3 and Source Code Pro fonts (Inter, JetBrains Mono and fonts from Google Fonts or Fontsource in Settings › Fonts), and the main menu (☰).
- The Islands Dark, Dark and Islands Light themes, each with its own syntax colors.
- The interface is in English and French. At the first start, Atelier takes the language of the system (English when it is not available); change it in **Settings › Appearance › Language**, without restarting.

## Projects

- The **Projects** page (File › All Projects…, the project widget or Search Everywhere): the last 50 projects with the date they were opened, a search box, and for each one: open here or in a new window, show in the file explorer, copy the path or remove from the list. A folder that no longer exists is greyed out.
- Several projects at once: each project opens in its own window. Opening a project asks for **This window** or **New window**. The project widget lists the recent projects; a right click opens one straight in a new window.
- Drag and drop: a file opens in a tab, a folder offers to open it as a project.

## Search Everywhere

**Double Shift** (**Ctrl+Shift+N** for files only, **Ctrl+Shift+A** for actions only) searches the files of the project, the open tabs, and the actions of the IDE and its plugins, with the keyboard. **Ctrl+N** goes to a symbol: the classes, functions and types of the project.

## Project tree

Collapsible folders, file creation, a context menu (copy the path, show in the file explorer, open in the terminal, Git actions) and manual refresh. The `.git`, `target`, `node_modules`, `.venv` and `__pycache__` folders are hidden. With Git, the tree is colored by status.

## Editor

- UTF-8 editor with tabs, syntax highlighting for HTML, CSS, JavaScript, TypeScript, Java, Python and Rust, line numbers, undo/redo and a marker for modified files.
- Open, save, save as and save all. Atelier asks before closing a modified tab, changing project or quitting. A change made on disk by another program blocks the ordinary save, so that it is not overwritten.
- **Find and replace**: **Ctrl+F** opens the search bar (the selection becomes the query; `Aa` match case, `W` whole words, `.*` regular expression, also Alt+C, Alt+W, Alt+X; “3 of 12”; matches highlighted; Enter / Shift+Enter or F3 / Shift+F3 to move). **Ctrl+R** adds replace (“Replace” and “Replace All”, undone at once by Ctrl+Z).
- **Find in project**: **Ctrl+Shift+F** opens the **Find** window: searches the whole project except dependencies, with the same options, a file mask (`*.js, src/*`), results grouped by file and highlighted (a click opens the place), and “Replace All” in the project after confirmation.
- **Code folding**: chevrons in the gutter (braces, brackets, parentheses, block comments, import groups). **Ctrl+-** folds, **Ctrl+=** unfolds, **Ctrl+Shift+-** / **Ctrl+Shift+=** fold / unfold all, **Ctrl+.** toggles. Clicking the `...` unfolds, and reaching a hidden line (search, navigation) unfolds what hides it.
- **Problems** (**Alt+6**): errors and warnings of the open documents, grouped by file, with filters and a direct jump to the line; a click on the counter of the status bar opens it. Colored marks along the right edge of the editor show where the problems of the file are, and lead there in one click.
- Files are limited to 4 MB, and binary files are refused (except those a [viewer](../tools/viewers) opens).

## Terminal

An interactive terminal: PowerShell on Windows, `/bin/sh` on Linux and macOS, through a PTY with VT100 parsing; other shells (PowerShell 7, Command Prompt, Git Bash, WSL distributions) are offered when they are installed. The shell keeps its state (`cd`, variables, processes).

- An input line for commands, or the direct keyboard mode for interactive programs, arrows and Ctrl+C. **Esc** gives the keyboard back to the IDE.
- Addresses (`https://…`) and project paths with line and column (`src/main.rs:12:5`, compiler errors, stack traces) are links: underlined on hover, a click opens them in the browser or in the editor, at the right line.
- Each terminal shows the memory and CPU use of what runs in it.
- Output is shown without ANSI colors, and only the current screen is shown (no scrollback yet).

## Notifications

Errors and Git operations show up as balloons, and the **Notifications** tool window on the right rail keeps them.

## Settings

**Ctrl+Alt+S** opens the settings: a tree of pages with a search box (Appearance & Behavior, Keymap, Editor, Plugins, Languages & Frameworks, Build, Execution, Deployment), breadcrumbs, and **OK**, **Cancel** and **Apply**. Pages change a draft: nothing changes before Apply or OK, Cancel forgets it, and **Reset** restores a page.

## Not there yet

A debugger, partial commits by hunk, a terminal scrollback and a plugin marketplace.
