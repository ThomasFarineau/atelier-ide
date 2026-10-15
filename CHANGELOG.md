# Changelog

Every version of Atelier IDE. The entries come from the files of `.changeset/` (see [Versions & releases](https://thomasfarineau.github.io/atelier-ide/contributing/releases)).

## 0.1.2 (2026-10-15)

### Features

#### Runtimes du projet et notebooks Jupyter

Le nouveau plugin **Runtimes & Jupyter** installe un Python, des JDK et des Node.js dans le projet (`.atelier/runtimes`), vérifiés par leur SHA-256. Les nouveaux terminaux du projet les ont en tête du `PATH` (avec `JAVA_HOME` et le `.venv` activé). Les versions épinglées par `.python-version`, `.java-version`, `.sdkmanrc`, `.nvmrc` ou `.node-version` sont lues et proposées à l'installation.

Les fichiers `.ipynb` s'ouvrent en notebook : cellules éditées et exécutées sur place (Shift+Entrée) avec le Python du projet, sorties texte, images et Markdown, et ouverture dans JupyterLab.

### Fixes

#### Ctrl+clic vers un autre fichier sans sélection parasite

Un Ctrl+clic qui ouvrait la déclaration dans un autre fichier sélectionnait du texte dans ce fichier si la souris bougeait avant le relâchement du bouton.

#### Renommer (Maj+F6) s'applique à nouveau avec Entrée

Dans le champ de Renommer, Entrée ne faisait rien : le renommage ne s'appliquait jamais.

#### Renommer un nom importé renomme aussi sa déclaration

En JavaScript et TypeScript, Renommer (Maj+F6) sur une fonction importée, depuis un fichier qui l'utilise, ne changeait que ce fichier : l'import ne correspondait plus à rien. Le fichier qui la déclare et les autres fichiers qui l'importent sont maintenant renommés aussi, comme quand on renomme depuis la déclaration.

#### Sélection à la souris, copie et molette dans le terminal

Une sélection à la souris, dans l'éditeur comme dans le terminal, commençait quelques pixels après l'endroit cliqué : le premier caractère manquait, et Ctrl+C copiait parfois une sélection vide dans le terminal. Elle commence maintenant là où le bouton a été enfoncé.

Dans le terminal, la molette fonctionne aussi dans les programmes qui prennent tout l'écran (éditeurs, pagers, IA) : elle leur est envoyée comme des événements de molette s'ils suivent la souris, sinon comme les flèches ↑ et ↓.

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
