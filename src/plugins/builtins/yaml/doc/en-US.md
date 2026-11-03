---
name: YAML / TOML
category: tools
icon: ../../../../assets/icons/extensions/toml+yaml+yml+ini+cfg+conf+env.svg
summary: Coloring and formatting of YAML and TOML, comments kept, and conversion of YAML to JSON.
tags: [yml, toml, json, format]
---

# YAML & TOML

The built-in **YAML / TOML** plugin colors `.yaml`, `.yml` and `.toml` files and reformats them (**Ctrl+Alt+L**). Turned off in the **Plugins** window, these files open as plain text.

## Reformat Code

Only the layout changes; the comments, the quotes and the order of the keys stay.

- **YAML**: two spaces per level, one space after `key:` and after the `-` of a list item, no spaces at the end of the lines, at most two blank lines in a row. A list written at the level of its key stays there. The text of block scalars (`|`, `>`) keeps its own indentation.
- **TOML**: no indentation before keys and tables, `key = value`, `[table]` without inner spaces, two spaces per level in the arrays that span lines. Multi-line strings stay as they are.

The file is read before and after: when the new layout would change its data, or when the file has a syntax error, nothing changes and the status bar says why.

## Convert to JSON

In the project tree, the context menu of a `.yaml` or `.yml` file has **Convert to JSON**: the JSON of the file is written next to it (`config.yml` gives `config.json`) and opened. Anchors, aliases and merge keys (`<<: *defaults`) are resolved; a file with several documents (`---`) gives an array of them. An existing JSON file is never overwritten.
