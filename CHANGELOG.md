# Changelog

Every version of Atelier IDE. The entries come from the files of `.changeset/` (see [Versions & releases](https://thomasfarineau.github.io/atelier-ide/contributing/releases)).

## 0.1.3 (2026-10-16)

### Features

#### Éditeur, navigation et intelligence du code d'IntelliJ

**Éditeur.** Plusieurs curseurs : Alt+J ajoute l'occurrence suivante de la sélection, Alt+Maj+J retire la dernière, Ctrl+Alt+Maj+J les prend toutes, Alt+Maj+clic ajoute ou retire un curseur. La sélection en colonne se fait en glissant avec Alt, ou après Alt+Maj+Inser. Ctrl+Maj+Haut/Bas déplace l'instruction entière, bloc compris. Ctrl+Alt+T entoure la sélection (accolades, `if`, `try`, guillemets…) et propose vos modèles. Le fichier `templates.json` du dossier de données ajoute des modèles de code à la complétion et à « Entourer de ». L'éditeur peut être partagé en deux (Partager à droite ou en bas, dans le menu des onglets), les lignes qui ouvrent les blocs restent en haut, le fil d'Ariane montre où est le curseur. F11 pose un signet, Maj+F11 les liste, Ctrl+Maj+1…9 en pose un numéroté et Ctrl+1…9 y va. L'historique local garde une copie à chaque enregistrement (« Afficher l'historique local ») et montre en choisissant une version ce que sa restauration changerait, avant de la restaurer. Les fichiers de 4 à 64 Mo s'ouvrent en lecture seule.

**Navigation.** Ctrl+Maj+A cherche une action, Ctrl+Maj+E liste les emplacements récents avec leur ligne, Ctrl+Tab change de fichier, Ctrl+Q affiche la documentation, Ctrl+Maj+I la définition, Ctrl+H et Ctrl+Alt+H les hiérarchies de types et d'appels. Ctrl+P ne sert plus à Rechercher partout : il affiche les paramètres de l'appel.

**Intelligence du code.** Les indications du serveur de langage (types, noms des arguments) s'affichent dans le code. Ctrl+P donne la signature de l'appel, avec le paramètre courant. Ctrl+Alt+Maj+T liste les refactorings du serveur pour la sélection. « Inspecter le code » cherche les erreurs de syntaxe de tous les fichiers du projet et les met dans Problèmes ; la fenêtre Commit prévient quand le code à valider en contient. L'orthographe des commentaires, des chaînes et des fichiers Markdown est vérifiée (anglais et français) : Alt+Entrée propose les mots proches et l'ajout au dictionnaire. Ces réglages sont dans Éditeur › Général.

**Correction.** Rechercher partout (double Maj) ne fige plus l'IDE sur un gros projet : la liste des fichiers se fait en arrière-plan et les recherches sont gardées d'une image à l'autre.

**Modèles.** Les modèles postfixes (`valeur.if`, `.var`, `.for`, `.return`, `.log`… selon le langage) prennent l'expression avant le point. Les modèles de code, postfixes et d'entourage de l'utilisateur se modifient dans Éditeur › Modèles de code. « Entourer de » connaît aussi les blocs de Python. Alt+Entrée, Ctrl+Alt+V/C/N/M : intentions et refactorings de l'IDE (extraire une variable, une constante, une méthode, inliner). « Inspecter le code » ajoute les erreurs des serveurs et de `tsc`.

### Fixes

#### Liens sur plusieurs lignes, icônes, sélection lisible et copie dans le terminal

Un lien qui tient sur plusieurs lignes s'ouvre en entier, qu'il soit coupé par le terminal ou par un programme qui coupe lui-même son texte, comme une IA : il est souligné sur toutes ses lignes.

La sélection est dessinée sous le texte, qui reste lisible.

Les icônes des invites et des outils en ligne de commande s'affichent avec une Nerd Font installée sur la machine, et les symboles manquants avec celle du système (Segoe UI Symbol sous Windows).

Un programme qui copie lui-même du texte (OSC 52), comme une IA en plein écran, le met dans le presse-papiers. Alt et une lettre envoient Échap puis la lettre : Alt+V colle une image dans une IA, Alt+B et Alt+F avancent d'un mot dans le shell.

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
