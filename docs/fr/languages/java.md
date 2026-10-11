# Java

Les fichiers `.java` suivent le même modèle : une analyse locale avec tree-sitter dès l'ouverture d'un fichier, puis **jdtls** (Eclipse JDT Language Server) pour tout le reste.

## Sans serveur

- Erreurs de syntaxe.
- Structure du fichier (**Ctrl+F12**) : classes, interfaces, enums, records, méthodes, constructeurs et champs.
- Occurrences, et la déclaration dans le fichier.
- Complétion des noms, des mots clés, des types courants et des modèles d'IntelliJ : `psvm`, `sout`, `fori`, `iter`, `ifn`, `try`, `test`…

## Avec jdtls

Complétion des membres, infobulles, erreurs du compilateur, navigation dans le projet et ses dépendances Maven ou Gradle, renommage, actions rapides, [Reformater le code et Optimiser les imports](./formatting).

Installez-le depuis Rechercher partout (**Java : installer le serveur de langage**) dans `~/.atelier/lsp/jdtls`. Il démarre avec Java 21 ou plus récent (`JAVA_HOME`, sinon le PATH) ; un `jdtls` du PATH convient aussi. Chaque projet a son propre espace de travail jdtls dans `~/.atelier/lsp/jdtls-data`.

## Exécution

**Maj+F10** exécute le fichier actif avec `java chemin/du/Main.java` (JDK avec le mode source, Java 11+). Pour un projet entier, voir [Maven et Gradle](../tools/maven-gradle).
