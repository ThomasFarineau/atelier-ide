---
aside: false
---

# Plugins

Atelier ships with a catalog of built-in plugins: compiled into the IDE, available in every project and **enabled by default**, except Git, which is on while the project has a `.git`: it comes on by itself after a `git init`. Search them below by name, file extension or tool (`docx`, `kubectl`, `pnpm`…), or filter by category.

<PluginMarketplace />

## How built-in plugins work

Each plugin is turned on or off in the **Plugins** window, for the open project: the state of every plugin is written in the project, in the `plugins` section of `.atelier/config.json`, and can be shared with it. `"auto"` leaves a plugin to the IDE (Git, until it is turned on or off by hand):

```json
{
  "plugins": {
    "builtin": { "git": "auto", "k8s": false, "markdown": true },
    "external": { "rust-tools": true }
  }
}
```

Plugins extend the interface itself: tool windows, title bar and status bar widgets, editor gutter, project tree colors, shortcuts and Search Everywhere commands. A plugin with nothing to do in the open project (Maven without a `pom.xml`…) hides its windows and commands.

A plugin can have **sub-plugins**, shown under it in the **Plugins** window: a sub-plugin runs only while its parent is on, and can be turned off by itself (GitHub, under Git).

The coloring of a language comes with its plugin (JavaScript, Rust, Java, Markdown, YAML / TOML, and Cargo for `Cargo.lock`): when the plugin is off, its files open as plain text. The other languages (HTML, CSS, JSON, Python, shell…) are colored by Atelier itself.

## External plugins

External plugins are JSON manifests of a project, limited to commands. They are enabled for each project separately, and no external plugin runs when it is loaded.

To try one, copy `examples/plugins/rust-tools.json` to `.atelier/plugins/rust-tools.json` in your project, click **Plugins**, then **Reload**, and enable the extension.

```json
{
  "id": "rust-tools",
  "name": "Rust Tools",
  "version": "1.0.0",
  "description": "Cargo commands for the current project",
  "commands": [
    { "name": "Check", "program": "cargo", "args": ["check"] },
    { "name": "Test", "program": "cargo", "args": ["test"] }
  ]
}
```

Arguments are passed as literal values, escaped for the shell. Commands run with the rights of the user: only enable manifests you trust. This API provides external commands, not native libraries.
