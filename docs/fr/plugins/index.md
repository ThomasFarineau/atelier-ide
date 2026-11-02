---
aside: false
---

# Plugins

Atelier est livré avec un catalogue de plugins intégrés : compilés dans l'IDE, disponibles dans tous les projets et **activés par défaut**. Recherchez-les ci-dessous par nom, extension de fichier ou outil (`docx`, `kubectl`, `pnpm`…), ou filtrez par catégorie.

<PluginMarketplace />

## Fonctionnement des plugins intégrés

Chaque plugin se désactive dans la fenêtre **Plugins**. Les plugins étendent l'interface elle-même : fenêtres d'outils, widgets de la barre de titre et de la barre d'état, gouttière de l'éditeur, couleurs de l'arborescence, raccourcis et commandes de Rechercher partout. Un plugin sans rien à faire dans le projet ouvert (Maven sans `pom.xml`…) masque ses fenêtres et ses commandes.

Un plugin peut avoir des **sous-plugins**, affichés sous lui dans la fenêtre **Plugins** : un sous-plugin ne fonctionne que quand son parent est activé, et se désactive à part (GitHub, sous Git).

La coloration d'un langage vient de son plugin (JavaScript, Rust, Java, Markdown, YAML / TOML, et Cargo pour `Cargo.lock`) : plugin désactivé, ses fichiers s'ouvrent en texte brut. Les autres langages (HTML, CSS, JSON, Python, shell…) sont colorés par Atelier lui-même.

## Plugins externes

Les plugins externes sont des manifestes JSON propres à un projet, limités à des commandes. Ils s'activent projet par projet, et aucun plugin externe ne s'exécute à son chargement.

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

Les arguments sont transmis comme des valeurs littérales, échappées pour le shell. Les commandes s'exécutent avec les droits de l'utilisateur : n'activez que des manifestes de confiance. Cette API fournit des commandes externes, pas des bibliothèques natives.
