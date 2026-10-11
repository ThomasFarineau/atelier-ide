# Reformat code

| Shortcut | Action |
| --- | --- |
| Ctrl+Alt+L | Reformats the active file, or only the selected lines (Code › Reformat Code) |
| Ctrl+Alt+Shift+L | Opens **Reformat File**: the whole file, the selected text, or only the text changed since the last commit (from the Git markers), with the **Optimize imports** option |
| Ctrl+Alt+O | Only optimizes the imports |

The status bar says how many lines changed, or that the code was already well formatted. The new text comes in as small edits: the caret stays on the same code, and a single **Ctrl+Z** undoes it all.

## By language

| Language | Formatter |
| --- | --- |
| Rust | rustfmt, through rust-analyzer when it runs, otherwise directly, with the edition of the crate and the project's `rustfmt.toml`. The line endings of the file are kept. |
| JavaScript, TypeScript | The formatter of tsserver (language server), with the indentation of the file. Optimizing the imports sorts them and removes the unused ones. |
| Java | jdtls |
| JSON | Atelier itself, with the indentation of the file and the order of the keys |
