# Cargo

The built-in Cargo plugin adds the **Cargo** window on the right rail, only in a project that has a `Cargo.toml`. It lists every `Cargo.toml` of the project (root, workspace members and subfolders, without `target`), with the targets Cargo finds in them:

- the `[[bin]]`, `[[example]]`, `[[test]]` and `[[bench]]` tables of the manifest;
- then those Cargo discovers by itself: `src/main.rs`, `src/bin/*.rs`, `examples/*.rs`, `tests/*.rs`, `benches/*.rs`, or a folder there with a `main.rs` (unless `autobins`, `autoexamples`, `autotests` or `autobenches` is `false`).

A manifest with a `[workspace]` table is marked **workspace**.

- **Double-click** (or ▶) on a target: it runs in a new terminal, from the folder of its `Cargo.toml` (`cargo run --bin app`, `cargo run --example demo`, `cargo test --test it`, `cargo bench --bench speed`).
- Context menu of a `Cargo.toml`: build, check, test, Clippy (`--all-targets`), format, open the documentation, update the dependencies, clean, open the file. Of a target: run it or open its source file.
- The toolbar builds the selected package (the root one otherwise) and runs the selected target; the version of `cargo` is shown on the right.
- Search Everywhere: **Cargo: Show Cargo Projects**, **Cargo: Build**, **Cargo: Check**, **Cargo: Test**.

What the window starts becomes a temporary [run configuration](../guide/running), to run again with Shift+F10.
