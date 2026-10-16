# Plugins

## Built-in plugins

Built-in plugins are compiled into the IDE, available in every project and enabled by default; each one can be turned off in the **Plugins** window. They extend the interface itself: tool windows, title bar and status bar widgets, editor gutter, project tree colors, shortcuts and Search Everywhere commands.

| Plugin | What it does |
| --- | --- |
| Git | [Git integration](../guide/git) |
| Markdown, Mermaid, SVG, CSV, Images, Microsoft Office | [File viewers](./viewers) |
| Node.js | [npm window](./node) |
| Cargo | [Cargo window](./cargo) |
| Runtimes | [Python, JDKs and Node.js in the project](./runtimes) |
| Jupyter | [Jupyter notebooks](./jupyter) |
| Maven, Gradle | [Maven and Gradle windows](./maven-gradle) |
| Translations | [Translations window](./translations) |
| Kubernetes | Browses the resources of a cluster, reads their description, YAML and logs, restarts and deletes them, with the installed `kubectl` and its kubeconfig |
| Atelier Super Intelligence Tools | The [MCP server](../ai/mcp) for AI agents |

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

Arguments are passed as literal values, escaped for the shell. Commands run with the rights of the user: only enable manifests you trust. This API provides external commands, not native libraries or a marketplace.
