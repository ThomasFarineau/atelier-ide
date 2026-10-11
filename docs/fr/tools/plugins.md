# Plugins

## Plugins intégrés

Les plugins intégrés sont compilés dans l'IDE, disponibles dans tous les projets et activés par défaut ; chacun se désactive dans la fenêtre **Plugins**. Ils étendent l'interface elle-même : fenêtres d'outils, widgets de la barre de titre et de la barre d'état, gouttière de l'éditeur, couleurs de l'arborescence, raccourcis et commandes de Rechercher partout.

| Plugin | Rôle |
| --- | --- |
| Git | [Intégration de Git](../guide/git) |
| Markdown, Mermaid, SVG, CSV, Images, Microsoft Office | [Visionneuses](./viewers) |
| Node.js | [Fenêtre npm](./node) |
| Maven, Gradle | [Fenêtres Maven et Gradle](./maven-gradle) |
| Traductions | [Fenêtre Traductions](./translations) |
| Kubernetes | Parcourt les ressources d'un cluster, affiche leur description, leur YAML et leurs logs, les redémarre et les supprime, avec le `kubectl` installé et son kubeconfig |
| Atelier Super Intelligence Tools | Le [serveur MCP](../ai/mcp) pour les agents IA |

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

Les arguments sont transmis comme des valeurs littérales, échappées pour le shell. Les commandes s'exécutent avec les droits de l'utilisateur : n'activez que des manifestes de confiance. Cette API fournit des commandes externes, pas des bibliothèques natives ni un marketplace.
