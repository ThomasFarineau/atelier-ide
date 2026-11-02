---
name: Gradle
summary: Les projets d’un build Gradle et leurs tâches par groupe, lancées avec gradlew ou gradle.
---

# Gradle

Le plugin intégré **Gradle** ajoute sa fenêtre sur le rail de droite, seulement dans un projet qui a un `settings.gradle` ou un `build.gradle`. Ailleurs, ni sa fenêtre ni ses commandes n'apparaissent.

Les projets du build (`settings.gradle(.kts)`, `rootProject.name` et `include`) et leurs **tâches par groupe**.

Avant tout appel à Gradle, la fenêtre montre les tâches des plugins du fichier de build (`java`, `application`, Spring Boot, Kotlin…) ; **Recharger** demande ensuite à Gradle la liste réelle (`tasks --all`), en arrière-plan. Un double-clic lance la tâche avec `gradlew`, sinon `gradle` (`:app:build` pour un sous-projet), avec le mode hors ligne s'il est coché. Le menu d'un projet affiche ses dépendances ou ouvre son fichier de build.

Ce que lance cette fenêtre devient une [configuration d'exécution](../guide/running) temporaire.
