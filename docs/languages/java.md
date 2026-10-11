# Java

`.java` files follow the same model: a local analysis with tree-sitter as soon as a file opens, then **jdtls** (Eclipse JDT Language Server) for everything else.

## Without a server

- Syntax errors.
- File structure (**Ctrl+F12**): classes, interfaces, enums, records, methods, constructors and fields.
- Occurrences, and the declaration in the file.
- Completion of names, keywords, common types, and IntelliJ's templates: `psvm`, `sout`, `fori`, `iter`, `ifn`, `try`, `test`…

## With jdtls

Member completion, tooltips, compiler errors, navigation in the project and its Maven or Gradle dependencies, rename, quick actions, [Reformat code and Optimize imports](./formatting).

Install it from Search Everywhere (**Java: Install Language Server**) in `~/.atelier/lsp/jdtls`. It starts with Java 21 or newer (`JAVA_HOME`, otherwise the PATH); a `jdtls` on the PATH works too. Each project has its own jdtls workspace in `~/.atelier/lsp/jdtls-data`.

## Running

**Shift+F10** runs the active file with `java path/to/Main.java` (a JDK with source mode, Java 11+). For a whole project, see [Maven & Gradle](../tools/maven-gradle).
