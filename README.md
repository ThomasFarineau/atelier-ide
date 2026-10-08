# Forge IDE

MVP d'IDE desktop écrit en Rust, avec une interface inspirée d'IntelliJ : barre d'outils, explorateur, onglets, éditeur central et terminal redimensionnable.

## Démarrage

Prérequis : Rust stable et une chaîne de compilation native. Sous Windows, installer les Build Tools Visual Studio avec le composant C++.

```powershell
cargo run
# Ouvrir directement un projet
cargo run -- "C:\chemin\vers\projet"
# Projet de démonstration multilangage
cargo run -- examples/demo
# Produire l'exécutable
cargo build --release
```

L'exécutable Windows est `target/release/forge-ide.exe`.

## Fonctionnalités

- Explorateur de projet avec dossiers pliables, création de fichiers et actualisation manuelle. Les dossiers `.git`, `target`, `node_modules`, `.venv` et `__pycache__` sont masqués.
- Éditeur UTF-8 avec onglets, coloration syntaxique, numéros de ligne, undo/redo, recherche exacte, sélection du résultat suivant et indicateur des modifications.
- HTML, CSS, JavaScript, TypeScript, Java, Python et Rust. La coloration TypeScript utilise la grammaire JavaScript pour ce MVP.
- Ouvrir, enregistrer, enregistrer sous et enregistrer tous les fichiers modifiés. Confirmation avant fermeture d'un onglet modifié, changement de projet ou sortie. Une modification externe sur disque bloque l'enregistrement ordinaire pour éviter de l'écraser.
- Trois thèmes : Forge Dark, Midnight et IntelliJ Light. Taille de police, projets récents et activation des plugins persistants via le stockage natif eframe.
- Terminal interactif PowerShell sous Windows, `/bin/sh` sous Linux/macOS, via PTY et analyse VT100. Le shell conserve son état (`cd`, variables, processus). Sortie affichée sans couleurs ANSI ; les séquences de positionnement sont interprétées. Zone de saisie pour les commandes ou mode clavier direct pour les programmes interactifs, les flèches et Ctrl+C. Échap rend le clavier à l'IDE.
- Plugins locaux à commandes, activés séparément pour chaque projet. Aucun plugin n'est exécuté au chargement.
- Palette de commandes et accès aux fichiers déjà ouverts.

## Exécution

F5 enregistre le fichier actif si nécessaire puis envoie une commande au shell du projet. Les outils doivent être installés et accessibles dans le `PATH`.

| Langage | Action |
| --- | --- |
| HTML | Ouvrir le fichier dans le navigateur par défaut |
| CSS | Édition ; ouvrir le HTML associé pour voir le résultat |
| JavaScript | `node chemin/du/fichier.js` |
| TypeScript | `tsx chemin/du/fichier.ts` (installer cet outil séparément) |
| Java | `java chemin/du/Main.java` (JDK avec mode source, Java 11+) |
| Python | `python -u chemin/du/fichier.py` |
| Rust | `cargo run` depuis la racine du projet Cargo |

Une session de terminal démarrée utilise la racine du projet. Après un `cd` manuel, les commandes suivantes utilisent ce nouveau répertoire ; **Redémarrer** revient à la racine du projet.

## Plugins

Copier `examples/plugins/rust-tools.json` dans `.forge/plugins/rust-tools.json` du projet souhaité, cliquer sur **Plugins**, puis **Recharger** et activer l'extension.

```json
{
  "id": "rust-tools",
  "name": "Rust Tools",
  "version": "1.0.0",
  "description": "Commandes Cargo pour le projet courant",
  "commands": [
    { "name": "Vérifier", "program": "cargo", "args": ["check"] },
    { "name": "Tester", "program": "cargo", "args": ["test"] }
  ]
}
```

Les arguments sont transmis comme des valeurs littérales, avec l'échappement du shell. Les commandes s'exécutent avec les droits de l'utilisateur ; activer uniquement des manifestes de confiance. Cette API fournit des commandes externes, pas des bibliothèques natives ou un marketplace.

## Raccourcis

| Raccourci | Action |
| --- | --- |
| Ctrl+O | Ouvrir un fichier |
| Ctrl+N | Nouveau document |
| Ctrl+S | Enregistrer |
| Ctrl+Shift+S | Enregistrer sous |
| Ctrl+W | Fermer l'onglet |
| Ctrl+F | Rechercher dans le fichier |
| Ctrl+P | Palette de commandes |
| Ctrl+` | Afficher/masquer le terminal |
| F5 | Exécuter |

Sur macOS, utiliser Cmd pour les raccourcis Ctrl de l'éditeur.

## Validation

```powershell
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

## Périmètre du MVP

L'éditeur permet de gérer des fichiers et d'exécuter les outils du projet. L'autocomplétion sémantique/LSP, le débogueur, les diagnostics inline, l'intégration Git, le rechargement automatique des fichiers et un marketplace de plugins restent à implémenter. La taille maximale d'un fichier est 4 Mo ; les fichiers binaires sont refusés. Le terminal affiche l'écran courant, sans navigation dans l'historique de sortie.

## Mode développement (hot reload)

```powershell
cargo dev            # alias de : cargo run -- --dev
cargo dev -- examples/demo
```

Le processus lancé surveille `src/`, `Cargo.toml` et `Cargo.lock`. À chaque modification, il recompile l'IDE dans `target/hot` ; si la compilation réussit, la fenêtre est remplacée par la nouvelle version en conservant le projet, les onglets, le texte non enregistré, l'onglet actif, les panneaux et la position de la fenêtre. En cas d'erreur de compilation, les messages s'affichent dans la console et l'IDE en cours continue de tourner. Le shell du terminal intégré est redémarré à chaque rechargement. Fermer la fenêtre arrête le mode dev.

## Architecture

- `src/app.rs` : interface, thèmes, raccourcis et cycle de vie des documents.
- `src/project.rs` : documents, détection des langages, accès aux fichiers.
- `src/terminal.rs` : pseudo-terminal, shell et écran VT100.
- `src/plugins.rs` : manifestes JSON et construction des commandes.
- `src/dev.rs` : mode développement, recompilation et rechargement à chaud de l'IDE.

Références techniques : [eframe](https://docs.rs/eframe/0.33.3/eframe/) et [portable-pty](https://docs.rs/portable-pty/0.9.0/portable_pty/).
