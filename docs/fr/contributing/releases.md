# Versions et releases

Les versions sont gérées par [Knope](https://knope.tech) (`knope.toml`), comme avec Changesets : ce sont les fichiers de `.changeset/` qui décident de la version et du changelog, pas les messages de commit.

## 1. Dans la pull request

Un changement qui mérite une ligne dans le changelog s'accompagne d'un fichier de changement, créé par `knope document-change` (type, puis résumé) ou écrit à la main dans `.changeset/<résumé>.md` :

```md
---
default: minor
---

# Le terminal affiche la mémoire et le CPU de chaque onglet

Détails facultatifs, en Markdown.
```

`major` pour un changement cassant, `minor` pour une nouveauté, `patch` pour une correction. Tant que la version est en `0.x`, `major` donne `0.(x+1).0`, et `minor` comme `patch` incrémentent le dernier chiffre.

## 2. Sur `main`

Le workflow **Prepare release** (`.github/workflows/prepare-release.yml`) ouvre ou met à jour la pull request « chore: prepare release X.Y.Z » depuis la branche `release`. Elle change la version de `Cargo.toml` et `Cargo.lock`, ajoute la section de `CHANGELOG.md` et supprime les fichiers de changement utilisés.

Le workflow compile ensuite la branche `release` : les fichiers sont à essayer dans les artefacts du run, et les dépendances compilées restent en cache pour la release.

::: warning
Cette pull request est poussée par GitHub Actions, donc la CI ne tourne pas dessus : un admin la merge sans attendre les vérifications obligatoires. Elle ne change que la version et le changelog.
:::

## 3. Au merge de cette pull request

Le workflow **Release** (`.github/workflows/release.yml`) compile l'IDE, son lanceur et les plugins natifs de `plugins/` pour Windows, Linux et macOS (Intel et Apple Silicon) avec `.github/workflows/build.yml`, ainsi que l'installateur Windows. Grâce au cache, seul le code d'Atelier est recompilé. La release est ensuite publiée à deux endroits :

- **[`atelier-ide-org/releases`](https://github.com/atelier-ide-org/releases)**, le dépôt public des téléchargements, où le lanceur cherche les mises à jour. Le code vit dans `atelier-ide-org/app`, qui deviendra privé. Cette étape demande le secret `RELEASES_TOKEN` dans `app` : un jeton qui peut écrire le contenu de `releases` (un jeton fine-grained limité à ce dépôt, ou une GitHub App de l'organisation). Sans lui, le workflow s'arrête avant de rien publier.
- **`atelier-ide-org/app`**, où `knope release` crée le tag `vX.Y.Z` et publie les notes du changelog. Les lanceurs installés avant le déménagement y cherchent encore, jusqu'à ce qu'ils se mettent à jour eux-mêmes (voir `MIGRATION.md`).

La release de `releases` reste un brouillon tant que tous les fichiers ne sont pas envoyés et que `knope` n'a pas fini, si bien que le lanceur n'en voit jamais une incomplète.

## Fichiers d'une release

| Fichier | Rôle |
| --- | --- |
| `atelier-setup-windows-x86_64.exe` | L'installateur Windows |
| `atelier-<os>-<arch>` | Le lanceur, téléchargé une fois sur Linux et macOS |
| `atelier-ide-<os>-<arch>.zip` / `.tar.gz` | L'archive qu'installe le lanceur : l'IDE et le prochain lanceur |
| `<plugin>-<os>-<arch>.zip` / `.tar.gz` | Un plugin natif (`gltf-viewer`) : son `plugin.json` et sa bibliothèque, à extraire dans `.atelier/plugins/<plugin>/` |
| `SHA256SUMS` | Les sommes de contrôle que vérifie le lanceur |

Un plugin natif ne se charge que dans l'IDE avec lequel il a été compilé : l'IDE et les plugins sortent d'un seul `cargo build`, et chaque release apporte ses propres plugins.

Le workflow **Build** se lance aussi à la main, pour compiler sans publier. L'organisation et le dépôt doivent autoriser « Allow GitHub Actions to create and approve pull requests » (Settings › Actions › General) pour que la pull request de release puisse être créée.
