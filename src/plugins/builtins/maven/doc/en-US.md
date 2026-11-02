---
name: Maven
category: build
summary: Lifecycle phases, plugins, goals, profiles and dependencies of the pom.xml, run with mvnw or mvn.
tags: [mvn, mvnw, pom.xml, java]
---

# Maven

The built-in **Maven** plugin adds its window on the right rail, only in a project that has a `pom.xml`. Elsewhere, neither its window nor its commands show up.

Every `pom.xml` of the project (root and modules, without `target`), each with:

- the **lifecycle** (`clean` to `deploy`);
- the **plugins** and their goals (those of the `pom.xml` and those of every project);
- the **profiles**, to tick;
- the declared **dependencies**.

A double-click runs the phase or the goal in a new terminal, with the project's `mvnw` wrapper, otherwise `mvn`, and `-f` pointing to the right `pom.xml`. The toolbar reloads the projects, runs a goal you type (`clean install`, `dependency:tree`…), and toggles offline mode (`-o`) and skip tests (`-DskipTests`).

What this window starts becomes a temporary [run configuration](../guide/running).
