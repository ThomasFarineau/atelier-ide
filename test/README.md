# Tests

Les tests Rust sont regroupés ici en suivant l'arborescence de `src/`.
Chaque fichier est chargé par son module source avec `#[cfg(test)]` et
`#[path = "..."]`, ce qui conserve l'accès aux fonctions privées et les noms
des tests.

Exécuter tous les tests depuis la racine du projet :

```sh
cargo test --locked
```

Les filtres existants restent valables :

```sh
cargo test --locked terminal::tests
```
