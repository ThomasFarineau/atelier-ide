# Maven et Gradle

Les plugins intégrés **Maven** et **Gradle** ajoutent leurs fenêtres sur le rail de droite, comme IntelliJ, seulement dans un projet qui a un `pom.xml` (Maven) ou un `settings.gradle` / `build.gradle` (Gradle). Ailleurs, ni leur fenêtre ni leurs commandes n'apparaissent.

## Maven

Tous les `pom.xml` du projet (racine et modules, sans `target`), chacun avec :

- le **cycle de vie** (`clean` à `deploy`) ;
- les **plugins** et leurs goals (ceux du `pom.xml` et ceux de tout projet) ;
- les **profils**, à cocher ;
- les **dépendances** déclarées.

Un double-clic lance la phase ou le goal dans un nouveau terminal, avec le wrapper `mvnw` du projet, sinon `mvn`, et `-f` vers le bon `pom.xml`. La barre d'outils recharge les projets, lance un goal saisi (`clean install`, `dependency:tree`…), et bascule le mode hors ligne (`-o`) et sans tests (`-DskipTests`).

## Gradle

Les projets du build (`settings.gradle(.kts)`, `rootProject.name` et `include`) et leurs **tâches par groupe**.

Avant tout appel à Gradle, la fenêtre montre les tâches des plugins du fichier de build (`java`, `application`, Spring Boot, Kotlin…) ; **Recharger** demande ensuite à Gradle la liste réelle (`tasks --all`), en arrière-plan. Un double-clic lance la tâche avec `gradlew`, sinon `gradle` (`:app:build` pour un sous-projet), avec le mode hors ligne s'il est coché. Le menu d'un projet affiche ses dépendances ou ouvre son fichier de build.

Ce que lancent ces fenêtres devient une [configuration d'exécution](../guide/running) temporaire.
