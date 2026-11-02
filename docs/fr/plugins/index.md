---
aside: false
---

# Plugins

Atelier est livré avec un catalogue de plugins intégrés : compilés dans l'IDE, disponibles dans tous les projets et **activés par défaut**, sauf Git, activé tant que le projet a un `.git` : il s'active de lui-même après un `git init`. Recherchez-les ci-dessous par nom, extension de fichier ou outil (`docx`, `kubectl`, `pnpm`…), ou filtrez par catégorie.

<PluginMarketplace />

## Fonctionnement des plugins intégrés

Chaque plugin s'active ou se désactive dans la fenêtre **Plugins**, pour le projet ouvert : l'état de chaque plugin est écrit dans le projet, dans la section `plugins` de `.atelier/config.json`, et peut se partager avec lui. `"auto"` laisse un plugin à l'IDE (Git, tant qu'on ne l'active ou ne le désactive pas à la main) :

```json
{
  "plugins": {
    "builtin": { "git": "auto", "k8s": false, "markdown": true },
    "external": { "rust-tools": true }
  }
}
```

Les plugins étendent l'interface elle-même : fenêtres d'outils, widgets de la barre de titre et de la barre d'état, gouttière de l'éditeur, couleurs de l'arborescence, raccourcis et commandes de Rechercher partout. Un plugin sans rien à faire dans le projet ouvert (Maven sans `pom.xml`…) masque ses fenêtres et ses commandes.

Un plugin peut avoir des **sous-plugins**, affichés sous lui dans la fenêtre **Plugins** : un sous-plugin ne fonctionne que quand son parent est activé, et se désactive à part (GitHub, sous Git).

La coloration d'un langage vient de son plugin (JavaScript, Rust, Java, Markdown, YAML / TOML, et Cargo pour `Cargo.lock`) : plugin désactivé, ses fichiers s'ouvrent en texte brut. Les autres langages (HTML, CSS, JSON, Python, shell…) sont colorés par Atelier lui-même.

## Plugins externes

Les plugins externes sont des manifestes JSON propres à un projet, qui apportent des commandes ou une bibliothèque native. Ils s'activent projet par projet, et aucun plugin externe ne s'exécute à son chargement.

Pour en essayer un, copiez `examples/plugins/rust-tools.json` dans `.atelier/plugins/rust-tools.json` de votre projet, cliquez sur **Plugins**, puis **Recharger**, et activez l'extension.

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

Les arguments sont transmis comme des valeurs littérales, échappées pour le shell. Les commandes s'exécutent avec les droits de l'utilisateur : n'activez que des manifestes de confiance.

### Plugins natifs

Un manifeste peut aussi apporter une bibliothèque dynamique, qui dessine dans l'IDE comme un plugin intégré : elle ouvre des fichiers dans un onglet de l'éditeur (la visionneuse de modèles 3D de `plugins/gltf-viewer`). Un tel plugin se range dans un dossier à lui, avec son manifeste nommé `plugin.json` et sa bibliothèque à côté :

```
.atelier/plugins/gltf-viewer/
  plugin.json
  gltf_viewer.dll        (libgltf_viewer.so sous Linux, libgltf_viewer.dylib sous macOS)
```

```json
{
  "id": "gltf-viewer",
  "name": "3D Model Viewer",
  "version": "0.1.0",
  "description": "glTF and GLB models shown in 3D, with their animations.",
  "library": "gltf_viewer"
}
```

Chaque release d'Atelier apporte ses plugins natifs, dans le [dépôt des téléchargements](https://github.com/atelier-ide-org/releases/releases) : prenez `gltf-viewer-<os>-<arch>` dans **la release de l'IDE que vous utilisez**, extrayez-le dans `.atelier/plugins/gltf-viewer/` de votre projet, puis cliquez sur **Plugins**, **Recharger**, et activez-le. Depuis les sources, `plugins/gltf-viewer/install.ps1` (ou `install.sh`) le compile et l'installe.

`library` est le nom de la bibliothèque, sans le préfixe ni l'extension du système. Elle n'est chargée qu'une fois le plugin activé, jamais avant, et s'exécute dans l'IDE avec les droits de l'utilisateur : n'activez que des bibliothèques de confiance.

Un plugin natif implémente le trait `Plugin` du crate `atelier-plugin-api` (`plugins/atelier-plugin-api`) et l'exporte avec `declare_plugin!`. Rust n'a pas d'ABI stable : la bibliothèque et l'IDE partagent egui, la bibliothèque doit donc être compilée par le même compilateur, pour la même cible et le même profil, avec les mêmes versions et fonctionnalités de leurs crates. Atelier le vérifie avant d'appeler la bibliothèque, et refuse celle qui a été compilée autrement. Les plugins de `plugins/` sont membres du workspace de l'IDE et se compilent avec lui : `cargo build --release` produit `target/release/gltf_viewer.dll` à côté d'`atelier-ide`.
