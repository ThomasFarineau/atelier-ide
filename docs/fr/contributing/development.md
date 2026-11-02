# Développement

## Compiler et vérifier

Il faut Rust stable et une chaîne de compilation native (sous Windows, les Build Tools de Visual Studio avec la charge de travail C++).

```bash
cargo run -- examples/demo   # l'IDE sur le projet de démonstration multilangage
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

La CI lance les mêmes vérifications sous Windows et Linux pour chaque pull request.

## Rechargement à chaud

```bash
cargo dev                    # alias de : cargo run -- --dev
cargo dev -- examples/demo
```

Le processus surveille `src/`, `Cargo.toml` et `Cargo.lock`. À chaque modification, il recompile l'IDE dans `target/hot` ; si la compilation réussit, la fenêtre est remplacée par la nouvelle version, en gardant le projet, les onglets, le texte non enregistré, l'onglet actif, les panneaux et la position de la fenêtre. En cas d'erreur de compilation, les messages s'affichent dans la console et l'IDE en cours continue de tourner. Le shell du terminal intégré redémarre à chaque rechargement. Fermer la fenêtre arrête le mode développement.

## Langues de l'interface

Les textes de l'interface sont dans des fichiers `locales/<identifiant>.json` (`en_US`, `fr_FR`) : celui de la racine pour l'IDE, et le dossier `locales` de chaque plugin ou langage pour ses propres textes (`src/plugins/builtins/git/locales/fr_FR.json`), sous des clés préfixées par son nom (`git.push`). `build.rs` les intègre à l'exécutable.

Dans le code, un texte s'écrit par sa clé : `tr!("save")`, ou `trf!("files_count", count = n)` pour un texte avec des valeurs (`"{count} fichiers"`). Une langue s'ajoute avec ses fichiers `<identifiant>.json`, dont la clé `language` donne le nom affiché dans les paramètres. Les tests vérifient que chaque langue a toutes les clés, avec les mêmes variables, et que le code n'utilise que des clés connues.

## Architecture

| Chemin | Contenu |
| --- | --- |
| `src/main.rs` | Point d'entrée de l'IDE |
| `src/app.rs`, `src/app/` | La coquille de l'IDE (barre de titre, rails, fenêtres d'outils, onglets, éditeur, barre d'état, dialogues), les raccourcis et le cycle de vie des documents ; les pages Projets, Paramètres et Plugins, les configurations d'exécution, Reformater le code, Aller au symbole |
| `src/ui.rs` | Le look and feel partagé avec les plugins : thèmes, polices, icônes, widgets |
| `src/highlight.rs` | Coloration syntaxique |
| `src/editor.rs`, `src/editor/` | L'éditeur : actions, popup de complétion et infobulles (`assist.rs`), repli du code |
| `src/languages/` | Intelligence du code : `mod.rs` (types, `Provider`, `Service`), `lsp/` (client du Language Server Protocol et fournisseur générique d'un langage à serveur), `javascript/`, `typescript/`, `rust/`, `java/`, `format.rs`, `syntax.rs` |
| `src/features/` | Fenêtre Problèmes et recherche dans le projet |
| `src/project.rs` | Documents, détection des langages, accès aux fichiers |
| `src/terminal*.rs`, `src/wsl.rs` | Pseudo-terminal, shells, écran VT100, liens, WSL |
| `src/plugins/` | `builtin.rs` (le contrat `BuiltinPlugin` et `Host`), `external.rs` (manifestes JSON) et `builtins/` (Git, Markdown, Mermaid, MCP, Node.js, Maven, Gradle, Traductions, Kubernetes, visionneuses…) |
| `src/graph/` | Le graphe de contexte |
| `src/i18n.rs`, `locales/` | Langues de l'interface |
| `src/cli.rs` | La commande `atelier <chemin>` et le relais vers un IDE déjà ouvert |
| `src/dev.rs` | Mode développement : recompilation et rechargement à chaud |
| `src/bin/atelier.rs`, `src/bin/atelier/` | Le lanceur (bootstrap) : mises à jour, fenêtre de démarrage |
| `installer/` | L'installateur Windows (Inno Setup) et la commande `atelier` |
| `build.rs` | Rassemble les icônes et les langues, et donne leur nom et leur icône aux exécutables Windows |
| `docs/` | Cette documentation (VitePress) |

## Documentation

La documentation est un site [VitePress](https://vitepress.dev) dans `docs/`, construit avec [Bun](https://bun.sh), en anglais (racine) et en français (`docs/fr/`, mêmes chemins). Chaque page existe dans les deux langues.

```bash
cd docs
bun install
bun run dev       # http://localhost:5173/atelier-ide/
bun run build     # vérifie aussi les liens
```

### Pages des plugins intégrés

Les pages et le catalogue des plugins intégrés (`/plugins/`, `/languages/`) ne sont pas dans `docs/` : chaque plugin les porte dans un dossier `doc`, à côté de son code, et le site les lit au build (`docs/.vitepress/catalog.ts`).

```
src/plugins/builtins/<plugin>/doc/     (src/languages/<langage>/doc/ pour un langage)
  en-US.md   la page, à /plugins/<plugin>
  fr-FR.md   la page en français, à /fr/plugins/<plugin>
```

Le front matter de chaque page est la fiche du plugin dans le catalogue :

```md
---
name: Cargo
category: build        # vcs, build, viewers, data, ai ou tools ; pas pour un langage
icon: icon.svg         # depuis le dossier du plugin ; icon.svg (et icon.dark.svg) par défaut
summary: "Packages, workspaces and targets of Cargo.toml: run, build, test, Clippy…"
tags: [rust, clippy]   # des mots de plus pour la recherche
see: git#github        # sans page : la page où mène la fiche
---

# Cargo
…
```

`fr-FR.md` ne donne que `name` et `summary` (et éventuellement des `tags` en plus) ; les autres champs sont dans `en-US.md`. Un fichier qui n'a qu'un front matter est une fiche sans page. L'IDE lit aussi le `name`, le `summary` et l'`icon`, pour sa page **Plugins** et ses menus (`build.rs` en fait les textes `plugin.<plugin>.name` et `plugin.<plugin>.summary`, et l'icône `card_icon("<plugin>")`) : un seul texte et un seul logo pour les deux. L'icône doit être une icône de l'IDE (un `.svg` que son build collecte). Le nom du dossier est l'adresse de la page, et un dossier de plugin dans un autre (`git/github`) est un sous-plugin, affiché dans la fiche de son parent. La barre latérale liste les pages d'elle-même ; une erreur dans une fiche arrête le build.

Elle est déployée sur GitHub Pages par `.github/workflows/docs.yml` à chaque push sur `main` qui touche `docs/`.
