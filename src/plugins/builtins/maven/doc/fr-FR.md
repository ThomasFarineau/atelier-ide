---
name: Maven
summary: Phases, plugins, goals, profils et dépendances des pom.xml, lancés avec mvnw ou mvn.
---

# Maven

Le plugin intégré **Maven** ajoute sa fenêtre sur le rail de droite, seulement dans un projet qui a un `pom.xml`. Ailleurs, ni sa fenêtre ni ses commandes n'apparaissent.

Tous les `pom.xml` du projet (racine et modules, sans `target`), chacun avec :

- le **cycle de vie** (`clean` à `deploy`) ;
- les **plugins** et leurs goals (ceux du `pom.xml` et ceux de tout projet) ;
- les **profils**, à cocher ;
- les **dépendances** déclarées.

Un double-clic lance la phase ou le goal dans un nouveau terminal, avec le wrapper `mvnw` du projet, sinon `mvn`, et `-f` vers le bon `pom.xml`. La barre d'outils recharge les projets, lance un goal saisi (`clean install`, `dependency:tree`…), et bascule le mode hors ligne (`-o`) et sans tests (`-DskipTests`).

Ce que lance cette fenêtre devient une [configuration d'exécution](../guide/running) temporaire.
