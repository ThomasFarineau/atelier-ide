# Rust

Les fichiers `.rs` fonctionnent comme JavaScript : une analyse locale avec tree-sitter dès l'ouverture d'un fichier, puis `rust-analyzer` quand il est installé.

## Sans serveur

- Erreurs de syntaxe.
- Structure du fichier (**Ctrl+F12**) : fonctions, structs et leurs champs, enums et leurs variantes, traits, blocs `impl`, modules, constantes, macros.
- Occurrences du nom sous le curseur, et **Ctrl+clic** vers la déclaration dans le fichier (variables locales, paramètres, éléments).
- Complétion des noms du fichier, du prélude, des mots clés et de modèles (`fn`, `test`, `tmod`, `impl`, `match`, `println`…).

## Avec rust-analyzer

- Complétion des membres après `.` et `::`, infobulles avec types et documentation.
- Erreurs du compilateur et `cargo check` à l'enregistrement.
- Navigation dans tout le projet et ses dépendances : déclaration, type, implémentations, utilisations.
- Renommage (**Maj+F6**) et actions rapides (**Alt+Entrée**).
- [Reformater le code](./formatting) avec rustfmt.

Atelier utilise le `rust-analyzer` de rustup (`rustup component add rust-analyzer`, aussi depuis Rechercher partout : **Rust : installer le serveur de langage**) ou celui du PATH. Un projet dont la racine n'a pas de `Cargo.toml` voit les crates de ses sous-dossiers déclarées au serveur.

## Exécution

**Maj+F10** lance `cargo run` depuis la racine du projet Cargo, et le widget d'exécution propose `cargo run`, `test`, `build` et `clippy` comme [configurations d'exécution](../guide/running).
