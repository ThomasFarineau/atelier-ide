# Graphe de contexte

Un plan du projet, fait **sans IA** et tenu à jour, que les agents lisent au lieu d'ouvrir des dizaines de fichiers, et qui leur fait consommer moins de contexte.

## L'activer

Dans un dépôt pour lequel vous n'avez pas encore décidé, la fenêtre Projet propose **Générer le graphe de contexte ?** ; le bouton graphe de son en-tête fait la même chose à tout moment (générer, ouvrir le rapport, régénérer, désactiver). La réponse est enregistrée dans `.atelier/config.json`.

Tant qu'il est activé, un fil d'exécution à part surveille les fichiers (à chaque enregistrement et toutes les 10 secondes), ne relit que ceux qui ont changé et reconstruit le graphe, en tâche d'arrière-plan visible dans la barre d'état.

## Ce qui est lu

- JavaScript et TypeScript ;
- Rust : fonctions, types, méthodes, imports, modules, appels, héritage ;
- les titres et les liens des fichiers Markdown.

Chaque lien est `EXTRACTED` (lu dans le code) ou `INFERRED` (déduit d'un nom unique dans le projet). Les fichiers qui s'appuient les uns sur les autres forment des groupes.

## Fichiers

Dans `.atelier/graph/` :

| Fichier | Contenu |
| --- | --- |
| `GRAPH_REPORT.md` | Le résumé à lire en premier : éléments centraux, groupes, et une estimation de la part de tokens économisés. Écrit en anglais, pour les agents IA. |
| `graph.json` | Le graphe, au format nœuds-liens |
| `cache.json` | Ce que disait chaque fichier, pour ne rien relire après un redémarrage |

Le dossier est ignoré par git (`.atelier/.gitignore`) : le graphe se régénère, il n'y a rien à partager. Supprimez la ligne pour le versionner.

## Pour les agents IA

Les agents l'interrogent par les outils MCP `graph_overview`, `graph_query`, `graph_node` et `graph_path`, dont les réponses s'arrêtent à un budget de tokens. La commande `commit` s'en sert aussi pour regrouper les fichiers (voir [Commits par l'IA](./commits)).
