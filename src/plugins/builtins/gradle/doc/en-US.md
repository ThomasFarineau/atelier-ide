---
name: Gradle
category: build
summary: The projects of a Gradle build and their tasks by group, run with gradlew or gradle.
tags: [gradlew, build.gradle, kotlin, java, android]
---

# Gradle

The built-in **Gradle** plugin adds its window on the right rail, only in a project that has a `settings.gradle` or a `build.gradle`. Elsewhere, neither its window nor its commands show up.

The projects of the build (`settings.gradle(.kts)`, `rootProject.name` and `include`) and their **tasks by group**.

Before any call to Gradle, the window shows the tasks of the plugins of the build file (`java`, `application`, Spring Boot, Kotlin…); **Reload** then asks Gradle for the real list (`tasks --all`), in the background. A double-click runs the task with `gradlew`, otherwise `gradle` (`:app:build` for a subproject), with offline mode if ticked. The menu of a project shows its dependencies or opens its build file.

What this window starts becomes a temporary [run configuration](../guide/running).
