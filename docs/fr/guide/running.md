# Exécuter du code

**Maj+F10** (ou **F5**) enregistre le fichier actif si besoin, puis envoie une commande au shell du projet. **Ctrl+F2** (ou le bouton stop) l'arrête. Les outils doivent être installés et accessibles dans le `PATH`.

## Le widget d'exécution

Le widget d'exécution de la barre de titre choisit ce que lance Maj+F10 : le **fichier actif** ou une **configuration d'exécution**.

### Le fichier actif

| Langage | Action |
| --- | --- |
| HTML | Ouvre le fichier dans le navigateur par défaut |
| CSS | Édition seulement ; ouvrez le HTML qui l'utilise pour voir le résultat |
| JavaScript | `node chemin/du/fichier.js` |
| TypeScript | `tsx chemin/du/fichier.ts` (installer `tsx` à part) |
| Java | `java chemin/du/Main.java` (JDK avec le mode source, Java 11+) |
| Python | `python -u chemin/du/fichier.py` |
| Rust | `cargo run` depuis la racine du projet Cargo |

### Configurations d'exécution

Une configuration est un nom, une commande tapée telle quelle dans le terminal et un dossier de travail relatif au projet. Elle tourne dans un onglet de terminal à son nom, que l'exécution suivante reprend.

**Modifier les configurations…** ouvre la liste : on y ajoute une commande shell ou une configuration proposée par le projet, on la copie, la renomme ou la supprime. Propositions :

- `cargo run`, `test`, `build` et `clippy` ;
- les scripts de `package.json`, avec le gestionnaire de son fichier de verrouillage ;
- `mvn package`, `test` et `clean install`, avec `mvnw` s'il est là ;
- `gradle build`, `test` et `run`, avec `gradlew` s'il est là.

Les configurations sont enregistrées dans `.atelier/run.json`, à partager avec le projet.

Ce que lancent les fenêtres Node.js, Maven et Gradle devient une configuration **temporaire** (en italique dans le widget, les cinq dernières). Relancée par Maj+F10, elle se garde d'un clic (**Enregistrer la configuration**).

## Dossier de travail

Une session de terminal démarre à la racine du projet. Après un `cd` manuel, les commandes suivantes s'exécutent dans ce dossier ; **Redémarrer** revient à la racine du projet.
