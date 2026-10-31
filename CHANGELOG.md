# Changelog

Every version of Atelier IDE. The entries come from the files of `.changeset/` (see [Versions & releases](https://thomasfarineau.github.io/atelier-ide/contributing/releases)).

## 0.1.4 (2026-10-31)

### Features

#### CSS, SCSS, Sass et Less

Les fichiers `.css`, `.scss`, `.sass` et `.less` sont colorés (sélecteurs, propriétés, variables, mixins, interpolations) et analysés sans serveur : erreurs de syntaxe, structure, complétion des propriétés, des valeurs, des couleurs, des pseudo-classes, des `@règles`, des variables et des mixins, aller à la déclaration et occurrences des variables, mixins, classes et keyframes. Le serveur de langage de VS Code (« CSS : installer » dans Rechercher partout, avec Node.js) ajoute les erreurs complètes, la documentation et le renommage pour CSS, SCSS et Less.

Une pastille s'affiche devant chaque couleur (`#hex`, `rgb()`, `hsl()`, noms). Un clic ouvre un sélecteur de couleur (teinte, opacité, couleur d'avant et d'après, hexadécimal, canaux, format d'écriture) qui modifie la couleur dans le texte ; un Ctrl+Z annule tous les changements faits avec le sélecteur.

Reformater le code (Ctrl+Alt+L) remet une déclaration et un sélecteur par ligne, même pour un fichier minifié. Le clic droit propose aussi **Minifier** (écrit `nom.min.css`) et **Transformer en SCSS, Less ou Sass** : variables, mixins, `@extend` et interpolations suivent, et les règles CSS qui se suivent sont imbriquées. Le fichier est écrit à côté, sans jamais en écraser un.

Ctrl+/ entoure les lignes CSS de `/* */`.

#### Fenêtres centrées, écran de démarrage et plantages expliqués

Une fenêtre s'ouvre toujours au milieu de l'écran où se trouve la souris, à sa taille par défaut ou à ce que l'écran permet. Elle ne revient plus toute petite dans le coin en haut à gauche après avoir été fermée réduite, et rouvre agrandie si la précédente l'était. Son bouton dans la barre des tâches suit la fenêtre quand elle passe sur un autre écran.

Chaque projet ouvert dans une nouvelle fenêtre est un programme à part : fermer la première fenêtre ne ferme plus les autres.

Lancé sans dossier, Atelier ouvre un écran de démarrage avec les projets récents, « Ouvrir… », « Cloner un dépôt… » et **« Nouveau projet… »** (un nom et un emplacement), au lieu d'ouvrir le dossier du programme.

Quand Atelier s'arrête de façon inattendue (erreur, mémoire saturée), la cause est écrite dans `logs/crash.log` du dossier de données et la fenêtre suivante le signale. Les lignes très longues (code minifié) ne sont affichées que sur leurs 10 000 premiers caractères : elles pouvaient prendre des gigaoctets de mémoire. Fermer l'onglet d'un gros fichier libère aussi ce que les plugins gardaient pour lui.

#### Lecteur multimédia : musique et vidéos

Les fichiers MP3, WAV, FLAC, OGG et M4A s'ouvrent dans un lecteur, sans rien à installer. Les vidéos MP4, MKV, AVI, MOV et WebM (et les sons comme Opus) sont lues avec ffmpeg : celui du PATH, ou un build que le lecteur propose de télécharger (Windows et Linux), vérifié par sa somme SHA-256.

Lecture et pause (Espace ou clic sur l'image), déplacement dans le fichier (barre de temps, ← et →), volume et son coupé. Un fichier se met en pause quand son onglet n'est plus affiché.

#### Menu contextuel de l'éditeur et « Ajouter au .gitignore »

Un clic droit dans le texte ouvre un menu : couper, copier, coller, tout sélectionner, étendre la sélection, commenter, dupliquer, reformater le code et, dans du code, les actions contextuelles, aller à la déclaration, trouver les utilisations, la documentation et renommer. Les commandes des plugins pour le fichier s'y ajoutent. Le clic droit garde la sélection quand il tombe dedans.

Dans l'arborescence du projet, le groupe Git du clic droit propose **« Ajouter au .gitignore »** pour un fichier, un dossier ou toute la sélection. Une notification prévient quand git suit déjà le fichier, que le `.gitignore` n'ignore alors pas.

#### Plugin Cargo, et Jupyter séparé de Runtimes

**Cargo.** Une fenêtre Cargo, sur le rail de droite des projets qui ont un `Cargo.toml`, liste les paquets et les workspaces du projet avec leurs cibles (binaires, exemples, tests, benchmarks). Un double-clic exécute une cible ; le menu d'un `Cargo.toml` compile, vérifie, teste, lance Clippy, formate, ouvre la documentation, met à jour les dépendances ou nettoie.

**Jupyter.** Les notebooks ont leur propre plugin, Jupyter, qui se désactive sans toucher aux environnements du projet ; le plugin Runtimes ne s'occupe plus que des Python, JDK et Node.js du projet et de son `.venv`.

#### Python

Sans serveur, Atelier connaît les fichiers Python par lui-même : erreurs de syntaxe, structure (classes, méthodes, fonctions, noms du module), occurrences d'un nom dans sa portée, aller à la déclaration (paramètres, affectations, boucles, imports, `self.x`), complétion des noms du fichier, des membres de `self`, des mots-clés et des fonctions intégrées, et modèles (`main`, `init`, `try`, `prop`…).

pyright (ou basedpyright, pris dans le venv du projet, le `PATH`, ou installé avec « Python : installer » dans Rechercher partout) prend le relais : types, erreurs, documentation, renommage, avec les paquets du `.venv` du projet. Reformater le code passe par ruff, sinon black. Les fichiers `.pyi` et `.pyw` sont aussi du Python.

### Fixes

#### Couleurs du code dans les template strings

En TypeScript, le code entre `${` et `}` d'une template string n'est plus coloré comme la chaîne : il garde les couleurs du code. En JavaScript, les appels de méthode (`liste.join(…)`) prennent la couleur des fonctions, comme en TypeScript.

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
