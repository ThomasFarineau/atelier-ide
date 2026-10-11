# Rust

`.rs` files work like JavaScript: a local analysis with tree-sitter as soon as a file opens, then `rust-analyzer` when it is installed.

## Without a server

- Syntax errors.
- File structure (**Ctrl+F12**): functions, structs and their fields, enums and their variants, traits, `impl` blocks, modules, constants, macros.
- Occurrences of the name under the caret, and **Ctrl+click** to the declaration in the file (local variables, parameters, items).
- Completion of the names of the file, the prelude, keywords and templates (`fn`, `test`, `tmod`, `impl`, `match`, `println`…).

## With rust-analyzer

- Completion of members after `.` and `::`, tooltips with types and documentation.
- Compiler errors and `cargo check` on save.
- Navigation in the whole project and its dependencies: declaration, type, implementations, usages.
- Rename (**Shift+F6**) and quick actions (**Alt+Enter**).
- [Reformat code](./formatting) with rustfmt.

Atelier uses the `rust-analyzer` of rustup (`rustup component add rust-analyzer`, also from Search Everywhere: **Rust: Install Language Server**) or the one on the PATH. A project whose root has no `Cargo.toml` gets the crates of its subfolders declared to the server.

## Running

**Shift+F10** runs `cargo run` from the root of the Cargo project, and the run widget offers `cargo run`, `test`, `build` and `clippy` as [run configurations](../guide/running).
