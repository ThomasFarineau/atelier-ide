# Commits par l'IA

Le serveur MCP propose des commandes (des « prompts » MCP) pour des commits qui suivent les conventions du dépôt. Dans Claude Code, ce sont `/mcp__atelier__commit`, `/mcp__atelier__rework-commit`…

## `commit`

Deux modes, aussi disponibles seuls en `commit-all` et `commit-by-context` :

- **all** : un seul commit avec tous les changements ;
- **context** : un commit par changement cohérent. Les fichiers qui s'appuient les uns sur les autres (via le [graphe de contexte](./context-graph), s'il est activé) ou d'un même dossier vont ensemble ; la doc, les dépendances (`Cargo.lock`, `package.json`…) et la CI ont chacune leur commit. Huit commits au plus.

L'agent passe par trois outils : `git_commit_plan` (les conventions du dépôt lues dans son historique, les fichiers et leur découpage), `git_diff` (ce qui a changé) et `git_commit` (les commits).

Les **conventions du dépôt** sont déduites de ses 60 derniers commits : Conventional Commits, gitmoji ou texte libre, français ou anglais, types et portées utilisés, majuscule, point final, longueur, présence d'un corps. Chaque message est vérifié avant tout commit : si un seul ne les respecte pas, rien n'est commité.

**Jamais de co-auteur** : les lignes `Co-authored-by` et les attributions à une IA sont retirées du message, qui qu'en soit l'auteur. Rien n'est poussé ni modifié après coup (pas d'amend).

## `rework-commit`

`/rework-commit last 10 commits` (aussi « since abc123 » ou `abc123..HEAD`) passe en revue les derniers commits et corrige les messages qui ne collent pas : le **titre** suit-il les conventions du dépôt et dit-il ce que fait vraiment le diff, et la **description** correspond-elle aux changements, sans rien affirmer de faux ?

`git_rework_plan` liste les commits (sujet, corps, fichiers modifiés) avec des constats automatiques :

- titre hors conventions ;
- type qui contredit les fichiers (`docs:` sur du code) ;
- portée absente de tous les chemins modifiés ;
- fichiers nommés dans le message mais non modifiés par le commit ;
- co-auteur ;
- gros changement sans description.

L'agent lit les diffs (`git_show`) et juge ; `git_rework_apply` ne réécrit que les messages choisis. Les arbres, les auteurs et les dates sont repris tels quels : aucun fichier ne bouge et aucun conflit n'est possible.

Garde-fous :

- pas de merge dans la chaîne, et un arbre de travail propre ;
- chaque message vérifié avant de toucher à quoi que ce soit ;
- **commits déjà poussés refusés**, sauf accord explicite de l'utilisateur (`allowPushed`) ;
- l'ancienne tête reste sous `refs/atelier/backup/<date>` (`git reset --hard <ref>` remet tout en place) ;
- jamais de co-auteur, jamais de push.
