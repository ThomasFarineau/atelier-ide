# Maven & Gradle

The built-in **Maven** and **Gradle** plugins add their windows on the right rail, as IntelliJ does, only in a project that has a `pom.xml` (Maven) or a `settings.gradle` / `build.gradle` (Gradle). Elsewhere, neither their window nor their commands show up.

## Maven

Every `pom.xml` of the project (root and modules, without `target`), each with:

- the **lifecycle** (`clean` to `deploy`);
- the **plugins** and their goals (those of the `pom.xml` and those of every project);
- the **profiles**, to tick;
- the declared **dependencies**.

A double-click runs the phase or the goal in a new terminal, with the project's `mvnw` wrapper, otherwise `mvn`, and `-f` pointing to the right `pom.xml`. The toolbar reloads the projects, runs a goal you type (`clean install`, `dependency:tree`…), and toggles offline mode (`-o`) and skip tests (`-DskipTests`).

## Gradle

The projects of the build (`settings.gradle(.kts)`, `rootProject.name` and `include`) and their **tasks by group**.

Before any call to Gradle, the window shows the tasks of the plugins of the build file (`java`, `application`, Spring Boot, Kotlin…); **Reload** then asks Gradle for the real list (`tasks --all`), in the background. A double-click runs the task with `gradlew`, otherwise `gradle` (`:app:build` for a subproject), with offline mode if ticked. The menu of a project shows its dependencies or opens its build file.

What these windows start becomes a temporary [run configuration](../guide/running).
