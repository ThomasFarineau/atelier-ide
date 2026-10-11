# Development

## Build and check

You need stable Rust and a native toolchain (on Windows, the Visual Studio Build Tools with the C++ workload).

```bash
cargo run -- examples/demo   # the IDE on the multi-language demo project
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The CI runs the same checks on Windows and Linux for every pull request.

## Hot reload

```bash
cargo dev                    # alias of: cargo run -- --dev
cargo dev -- examples/demo
```

The process watches `src/`, `Cargo.toml` and `Cargo.lock`. On each change it rebuilds the IDE in `target/hot`; when the build succeeds, the window is replaced by the new version, keeping the project, the tabs, unsaved text, the active tab, the panels and the window position. On a build error, the messages show in the console and the running IDE goes on. The shell of the built-in terminal restarts at each reload. Closing the window stops dev mode.

## Interface languages

The texts of the interface live in `locales/<identifier>.json` files (`en_US`, `fr_FR`): the one at the root for the IDE, and the `locales` folder of each plugin or language for its own texts (`src/plugins/builtins/git/locales/fr_FR.json`), under keys prefixed by its name (`git.push`). `build.rs` embeds them in the executable.

In the code, a text is named by its key: `tr!("save")`, or `trf!("files_count", count = n)` for a text with values (`"{count} files"`). A language is added with its `<identifier>.json` files, whose `language` key is the name shown in the settings. Tests check that every language has every key, with the same placeholders, and that the code only uses known keys.

## Architecture

| Path | Content |
| --- | --- |
| `src/main.rs` | Entry point of the IDE |
| `src/app.rs`, `src/app/` | The shell of the IDE (title bar, rails, tool windows, tabs, editor, status bar, dialogs), shortcuts and document life cycle; the Projects, Settings and Plugins pages, run configurations, Reformat code, Go to symbol |
| `src/ui.rs` | IntelliJ look and feel shared with the plugins: themes, fonts, icons, widgets |
| `src/highlight.rs` | Syntax highlighting in IntelliJ's colors |
| `src/editor.rs`, `src/editor/` | The editor: actions, completion popup and tooltips (`assist.rs`), folding |
| `src/languages/` | Code intelligence: `mod.rs` (types, `Provider`, `Service`), `lsp/` (Language Server Protocol client and the generic provider of a language with a server), `javascript/`, `typescript/`, `rust/`, `java/`, `format.rs`, `syntax.rs` |
| `src/features/` | Problems window and project search |
| `src/project.rs` | Documents, language detection, file access |
| `src/terminal*.rs`, `src/wsl.rs` | Pseudo-terminal, shells, VT100 screen, links, WSL |
| `src/plugins/` | `builtin.rs` (the `BuiltinPlugin` and `Host` contract), `external.rs` (JSON manifests) and `builtins/` (Git, Markdown, Mermaid, MCP, Node.js, Maven, Gradle, Translations, Kubernetes, viewers…) |
| `src/graph/` | The context graph |
| `src/i18n.rs`, `locales/` | Interface languages |
| `src/cli.rs` | The `atelier <path>` command and the hand-off to a running IDE |
| `src/dev.rs` | Dev mode: rebuild and hot reload |
| `src/bin/atelier.rs`, `src/bin/atelier/` | The launcher (bootstrap): updates, startup window |
| `installer/` | The Windows installer (Inno Setup) and the `atelier` command |
| `build.rs` | Collects the icons and the locales, and gives the Windows executables their name and icon |
| `docs/` | This documentation (VitePress) |

## Documentation

The documentation is a [VitePress](https://vitepress.dev) site in `docs/`, built with [Bun](https://bun.sh), in English (root) and French (`docs/fr/`, same paths). Each page exists in both languages.

```bash
cd docs
bun install
bun run dev       # http://localhost:5173/atelier-ide/
bun run build     # checks the links too
```

It is deployed to GitHub Pages by `.github/workflows/docs.yml` on every push to `main` that touches `docs/`.
