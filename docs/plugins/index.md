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

External plugins are JSON manifests of a project, which bring commands or a native library. They are enabled for each project separately, and no external plugin runs when it is loaded.

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

Arguments are passed as literal values, escaped for the shell. Commands run with the rights of the user: only enable manifests you trust.

### Native plugins

A manifest can also bring a dynamic library, which draws inside the IDE like a built-in plugin: it opens files in an editor tab (the 3D model viewer of `plugins/gltf-viewer`). Such a plugin goes in a folder of its own, with its manifest named `plugin.json` and its library next to it:

```
.atelier/plugins/gltf-viewer/
  plugin.json
  gltf_viewer.dll        (libgltf_viewer.so on Linux, libgltf_viewer.dylib on macOS)
```

```json
{
  "id": "gltf-viewer",
  "name": "3D Model Viewer",
  "version": "0.1.0",
  "description": "glTF and GLB models shown in 3D, with their animations.",
  "library": "gltf_viewer"
}
```

Each release of Atelier carries its native plugins, in the [repository of the downloads](https://github.com/atelier-ide-org/releases/releases): take `gltf-viewer-<os>-<arch>` from **the release of the IDE you run**, extract it into `.atelier/plugins/gltf-viewer/` of your project, then click **Plugins**, **Reload**, and enable it. From the sources, `plugins/gltf-viewer/install.ps1` (or `install.sh`) builds and installs it.

`library` is the name of the library without the prefix and extension of the system. It is loaded once the plugin is enabled, never before, and runs inside the IDE with the rights of the user: only enable libraries you trust.

A native plugin implements the `Plugin` trait of the `atelier-plugin-api` crate (`plugins/atelier-plugin-api`) and exports it with `declare_plugin!`. Rust has no stable ABI: the library and the IDE share egui, so the library must be built by the same compiler, for the same target and profile, from the same versions and features of their crates. Atelier checks it before calling the library, and refuses one built otherwise. The plugins of `plugins/` are members of the workspace of the IDE and are built with it: `cargo build --release` builds `target/release/gltf_viewer.dll` beside `atelier-ide`.
