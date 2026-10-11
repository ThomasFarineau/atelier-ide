# Running code

**Shift+F10** (or **F5**) saves the active file if needed, then sends a command to the shell of the project. **Ctrl+F2** (or the stop button) stops it. The tools must be installed and on the `PATH`.

## The run widget

The run widget of the title bar chooses what Shift+F10 starts: the **active file** or a **run configuration**.

### The active file

| Language | Action |
| --- | --- |
| HTML | Opens the file in the default browser |
| CSS | Editing only; open the HTML file that uses it to see the result |
| JavaScript | `node path/to/file.js` |
| TypeScript | `tsx path/to/file.ts` (install `tsx` separately) |
| Java | `java path/to/Main.java` (a JDK with source mode, Java 11+) |
| Python | `python -u path/to/file.py` |
| Rust | `cargo run` from the root of the Cargo project |

### Run configurations

A configuration is a name, a command typed as is in the terminal, and a working folder relative to the project. It runs in a terminal tab named after it, which the next run reuses.

**Edit Configurations…** opens the list: add a shell command or a configuration the project suggests, copy, rename or delete one. Suggestions:

- `cargo run`, `test`, `build` and `clippy`;
- the scripts of `package.json`, with the package manager of its lock file;
- `mvn package`, `test` and `clean install`, with `mvnw` when it is there;
- `gradle build`, `test` and `run`, with `gradlew` when it is there.

Configurations are saved in `.atelier/run.json`, to share with the project.

What the Node.js, Maven and Gradle windows start becomes a **temporary** configuration (in italics in the widget, the last five). Run again with Shift+F10, it is kept with one click (**Save Configuration**).

## Working folder

A terminal session starts at the root of the project. After a manual `cd`, the next commands run in that folder; **Restart** goes back to the root of the project.
