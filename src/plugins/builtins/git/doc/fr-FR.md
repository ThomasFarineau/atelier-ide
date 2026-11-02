---
name: Git
summary: Commit, branches, historique, diff, push/pull, stash, conflits et annotations, avec le git
  installé.
---

# Git

Le plugin intégré Git utilise la commande `git` installée (2.26 ou plus récente).

## Fenêtre Commit

**Alt+0**, sur le rail de gauche : modifications, conflits et fichiers non versionnés, avec des cases à cocher. Seuls les fichiers cochés sont commités, sans passer par l'index.

- Options **Modifier le dernier commit** (amend) et **Sign-off**, **Commit** et **Commit et pousser…**.
- Menu contextuel : diff, ouvrir, historique, annuler les modifications, ajouter à Git ou au `.gitignore`, copier le chemin.
- Onglet **Stash** : mettre de côté, appliquer, restaurer, supprimer, et voir les fichiers d'un stash.

## Fenêtre Git

**Alt+9**, sur le rail du bas : le log avec le graphe des branches, les badges des branches et des tags, des filtres par texte, hash, auteur ou branche, et l'historique d'un fichier. Le détail d'un commit affiche son message et ses fichiers.

Actions : checkout, nouvelle branche, tag, cherry-pick, revert, reset (soft, mixed, hard, keep) et annulation du dernier commit. L'onglet **Console** liste les commandes git lancées par l'IDE et leur sortie.

## Widget de branche

Dans la barre de titre : la branche courante, les commits en avance et en retard, l'opération en cours. Il donne accès :

- aux branches locales et distantes, au checkout intelligent (stash, checkout puis restauration), à la création, au renommage et à la suppression de branches, au merge et au rebase ;
- à la mise à jour (pull merge ou rebase, avec autostash), au push (commits sortants, force-with-lease, tags), au fetch ;
- au checkout d'un tag ou d'une révision, et aux remotes.

## Diff et conflits

- **Diff** côte à côte ou unifié, avec navigation entre les différences (**F7** / **Maj+F7**) et masquage des lignes identiques.
- **Conflits** : accepter la version locale ou entrante, marquer comme résolu, ou résoudre bloc par bloc dans la fenêtre de résolution. Un merge, un rebase ou un cherry-pick en cours peut être abandonné ou poursuivi.

## Dans l'éditeur et l'arborescence

Des marqueurs pour les lignes ajoutées, modifiées ou supprimées depuis HEAD, et les annotations **Git Blame**, par un clic droit sur les numéros de ligne. L'arborescence du projet est colorée selon le statut Git.

## Et aussi

- Créer un dépôt ou en cloner un, depuis le widget ou Rechercher partout.
- Les opérations tournent en arrière-plan, et le statut se rafraîchit toutes les 5 secondes ou quand on revient dans la fenêtre.
- Les fichiers ouverts sont enregistrés avant un commit, un checkout ou une mise à jour, puis rechargés s'ils ont changé sur le disque.
- Le plugin est automatique (`"git": "auto"` dans `.atelier/config.json`) : activé tant que le projet a un `.git` (un dossier, ou le fichier d'un worktree ou d'un sous-module), il s'active donc de lui-même après un `git init`, et désactivé sans. L'activer ou le désactiver dans la fenêtre Plugins en décide pour le projet.

| Raccourci | Action |
| --- | --- |
| Ctrl+K | Commit |
| Ctrl+Maj+K | Pousser |
| Ctrl+T | Mettre à jour le projet (pull) |
| Alt+0 / Alt+9 | Fenêtres Commit / Git |
| F7 / Maj+F7 | Différence suivante / précédente |

## GitHub

**GitHub** est un sous-plugin de Git : il ne fonctionne que quand Git est activé, et se désactive à part dans la fenêtre **Plugins**. Il apparaît dans les projets dont le dépôt a un remote GitHub, ou un dossier `.github`.

- La fenêtre **Pull Request**, sur le rail de droite, montre la pull request de la branche courante : son état, la décision des revues, chaque revue avec son commentaire, et les commentaires des revues sur le code, par conversation. Une conversation ouvre son fichier à sa ligne ; celle dont le code a changé depuis est marquée *obsolète*. **Actualiser** la relit, **Ouvrir sur GitHub** l'ouvre dans le navigateur.
- Elle passe par la CLI GitHub : installez [`gh`](https://cli.github.com) et connectez-vous avec `gh auth login`.
- Il apporte les [schémas](../languages/json#schemas) des workflows de GitHub Actions (`.github/workflows/*.yml`) et de `.github/dependabot.yml` : leurs valeurs sont vérifiées, leurs clés complétées et décrites sous la souris.
- Rechercher partout : **GitHub : afficher la pull request**.
