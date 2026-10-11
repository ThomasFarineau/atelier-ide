# Ligne de commande

Comme `code` pour VS Code, la commande `atelier` ouvre des dossiers et des fichiers dans l'IDE :

```bash
atelier .              # le dossier devient le projet
atelier src/main.rs    # le fichier s'ouvre dans l'IDE déjà ouvert
atelier -n ~/autre     # force une nouvelle fenêtre
```

Si un IDE est déjà ouvert, la commande lui confie le chemin (à la fenêtre dont le projet contient le fichier, sinon à la dernière utilisée) et se termine tout de suite ; un dossier passe par le choix « cette fenêtre / nouvelle fenêtre ». Sans IDE ouvert, elle en lance un.

La commande reste silencieuse et instantanée : elle n'affiche pas de fenêtre de démarrage, et ne cherche une mise à jour qu'au plus toutes les six heures.

## D'où vient la commande

| Installation | Commande |
| --- | --- |
| Installateur Windows | `bin\atelier.cmd` (cmd, PowerShell) et `bin\atelier` (Git Bash). Seul le dossier `bin` de l'installation va dans le PATH, comme pour VS Code. |
| Lanceur sur Linux ou macOS | Un lien `~/.local/bin/atelier` créé à la première installation |
| Lanceur sous Windows, sans l'installateur | Le dossier du lanceur, ajouté au PATH de l'utilisateur à la première installation |

`atelier --install-cli` remet la commande dans le PATH. Ouvrez ensuite un nouveau terminal : le PATH d'un terminal est lu à son démarrage.
