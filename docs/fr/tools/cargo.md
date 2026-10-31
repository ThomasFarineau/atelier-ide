# Cargo

Le plugin intégré Cargo ajoute la fenêtre **Cargo** sur le rail de droite, seulement dans un projet qui a un `Cargo.toml`. Elle liste tous les `Cargo.toml` du projet (racine, membres du workspace et sous-dossiers, sans `target`), avec les cibles que Cargo y trouve :

- les tables `[[bin]]`, `[[example]]`, `[[test]]` et `[[bench]]` du manifeste ;
- puis celles que Cargo découvre seul : `src/main.rs`, `src/bin/*.rs`, `examples/*.rs`, `tests/*.rs`, `benches/*.rs`, ou un dossier de ceux-là avec un `main.rs` (sauf si `autobins`, `autoexamples`, `autotests` ou `autobenches` vaut `false`).

Un manifeste qui a une table `[workspace]` est marqué **workspace**.

- **Double-clic** (ou ▶) sur une cible : elle s'exécute dans un nouveau terminal, depuis le dossier de son `Cargo.toml` (`cargo run --bin app`, `cargo run --example demo`, `cargo test --test it`, `cargo bench --bench speed`).
- Menu contextuel d'un `Cargo.toml` : compiler, vérifier, tester, Clippy (`--all-targets`), formater, ouvrir la documentation, mettre à jour les dépendances, nettoyer, ouvrir le fichier. D'une cible : l'exécuter ou ouvrir son fichier source.
- La barre d'outils compile le paquet sélectionné (sinon celui de la racine) et exécute la cible sélectionnée ; la version de `cargo` est affichée à droite.
- Rechercher partout : **Cargo : afficher les projets Cargo**, **Cargo : compiler**, **Cargo : vérifier**, **Cargo : tester**.

Ce que lance la fenêtre devient une [configuration d'exécution](../guide/running) temporaire, à relancer avec Maj+F10.
