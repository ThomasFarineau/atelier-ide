# Changelog

Every version of Atelier IDE. The entries come from the files of `.changeset/` (see [Versions & releases](https://thomasfarineau.github.io/atelier-ide/contributing/releases)).

## 0.1.1 (2026-10-11)

### Features

#### Installateur Windows, commande atelier, fenêtre de démarrage et nouveau logo

Sous Windows, `atelier-setup-windows-x86_64.exe` installe Atelier sans droits administrateur : raccourci dans le menu Démarrer, désinstalleur, et l'IDE déjà prêt sans téléchargement au premier démarrage. Comme VS Code, il met la commande `atelier` dans le PATH (cmd, PowerShell et Git Bash) : `atelier .` ouvre le dossier courant.

Comme Discord ou VS Code, le lanceur affiche une fenêtre avec le logo pendant qu'il cherche une mise à jour, la télécharge (avec sa progression) et l'installe, puis démarre l'IDE. En cas d'échec, elle explique pourquoi et propose de réessayer.

Aucune fenêtre de console ne s'ouvre plus au démarrage (installation de la commande `atelier`, liste des distributions WSL). Quand aucune release n'est trouvée, le message explique pourquoi (dépôt privé : `ATELIER_GITHUB_TOKEN`, ou pas encore de release) au lieu d'afficher « http status 404 ».

## 0.1.0 (2026-10-11)

### Breaking Changes

#### Première version publiée

Éditeur multilangage à l'interface d'IntelliJ, avec Git, terminal, plugins, serveur MCP et bootstrap de mise à jour pour Windows, Linux et macOS.
