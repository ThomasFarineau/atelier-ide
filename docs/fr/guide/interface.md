# Interface

Atelier a une interface moderne et épurée : barre de titre avec les widgets de projet, de branche et d'exécution, rails de fenêtres d'outils, îlots arrondis, onglets, Rechercher partout, notifications et barre d'état avec fil d'Ariane.

## Apparence

- Des icônes nettes, les polices Inter et JetBrains Mono, et le menu principal (☰).
- Les thèmes Islands Dark, Dark et Islands Light, chacun avec ses couleurs de coloration syntaxique.
- L'interface existe en anglais et en français. Au premier lancement, Atelier prend la langue du système (l'anglais si elle n'est pas disponible) ; elle se change dans **Paramètres › Apparence › Langue**, sans redémarrer.

## Projets

- La page **Projets** (Fichier › Tous les projets…, le widget de projet ou Rechercher partout) : les 50 derniers projets avec leur date d'ouverture, une recherche, et pour chacun : ouvrir ici ou dans une nouvelle fenêtre, afficher dans l'explorateur, copier le chemin ou retirer de la liste. Un dossier qui n'existe plus est grisé.
- Plusieurs projets à la fois : chaque projet s'ouvre dans sa propre fenêtre. L'ouverture d'un projet demande **Cette fenêtre** ou **Nouvelle fenêtre**. Le widget de projet liste les projets récents ; un clic droit en ouvre un directement dans une nouvelle fenêtre.
- Glisser-déposer : un fichier s'ouvre dans un onglet, un dossier propose de l'ouvrir comme projet.

## Rechercher partout

**Double Maj** ou **Ctrl+P** (**Ctrl+Maj+N** pour les fichiers seulement) cherche dans les fichiers du projet, les onglets ouverts et les actions de l'IDE et de ses plugins, au clavier. **Ctrl+N** va à un symbole : les classes, fonctions et types du projet.

## Arborescence du projet

Dossiers pliables, création de fichiers, menu contextuel (copier le chemin, afficher dans l'explorateur, ouvrir dans le terminal, actions Git) et actualisation manuelle. Les dossiers `.git`, `target`, `node_modules`, `.venv` et `__pycache__` sont masqués. Avec Git, l'arborescence est colorée selon le statut.

## Éditeur

- Éditeur UTF-8 avec onglets, coloration syntaxique pour HTML, CSS, JavaScript, TypeScript, Java, Python et Rust, numéros de ligne, annuler/rétablir et indicateur des fichiers modifiés.
- Ouvrir, enregistrer, enregistrer sous et tout enregistrer. Atelier demande confirmation avant de fermer un onglet modifié, de changer de projet ou de quitter. Une modification faite sur le disque par un autre programme bloque l'enregistrement ordinaire, pour ne pas l'écraser.
- **Rechercher et remplacer** : **Ctrl+F** ouvre la barre de recherche (la sélection devient la requête ; `Aa` respecter la casse, `W` mots entiers, `.*` expression régulière, aussi Alt+C, Alt+W, Alt+X ; « 3 sur 12 » ; occurrences surlignées ; Entrée / Maj+Entrée ou F3 / Maj+F3 pour avancer). **Ctrl+R** ajoute le remplacement (« Remplacer » et « Tout remplacer », annulable d'un coup par Ctrl+Z).
- **Rechercher dans le projet** : **Ctrl+Maj+F** ouvre la fenêtre **Rechercher** : recherche dans tout le projet hors dépendances, avec les mêmes options, un masque de fichiers (`*.js, src/*`), les résultats groupés par fichier et surlignés (un clic ouvre l'endroit), et « Tout remplacer » dans le projet après confirmation.
- **Repli du code** : chevrons dans la gouttière (accolades, crochets, parenthèses, commentaires de bloc, groupes d'imports). **Ctrl+-** replie, **Ctrl+=** déplie, **Ctrl+Maj+-** / **Ctrl+Maj+=** replient / déplient tout, **Ctrl+.** bascule. Un clic sur le `...` déplie, et atteindre une ligne cachée (recherche, navigation) déplie ce qui la cache.
- **Problèmes** (**Alt+6**) : les erreurs et avertissements des documents ouverts, groupés par fichier, avec filtres et accès direct à la ligne ; un clic sur le compteur de la barre d'état ouvre la fenêtre. Des marques colorées, le long du bord droit de l'éditeur, montrent où sont les problèmes du fichier et y mènent d'un clic.
- Les fichiers sont limités à 4 Mo, et les fichiers binaires sont refusés (sauf ceux qu'ouvre une [visionneuse](../tools/viewers)).

## Terminal

Un terminal interactif : PowerShell sous Windows, `/bin/sh` sous Linux et macOS, via un PTY et l'analyse VT100 ; d'autres shells (PowerShell 7, Invite de commandes, Git Bash, distributions WSL) sont proposés quand ils sont installés. Le shell garde son état (`cd`, variables, processus).

- Une zone de saisie pour les commandes, ou le mode clavier direct pour les programmes interactifs, les flèches et Ctrl+C. **Échap** rend le clavier à l'IDE.
- Les adresses (`https://…`) et les chemins du projet avec ligne et colonne (`src/main.rs:12:5`, erreurs de compilation, piles d'appels) sont des liens : soulignés au survol, ils s'ouvrent d'un clic dans le navigateur ou dans l'éditeur, à la bonne ligne.
- Chaque terminal affiche la mémoire et le CPU utilisés par ce qui tourne dedans.
- La sortie s'affiche sans couleurs ANSI, et seul l'écran courant est visible (pas encore d'historique de défilement).

## Notifications

Les erreurs et les opérations Git apparaissent en bulles, et la fenêtre d'outils **Notifications**, sur le rail de droite, les conserve.

## Paramètres

**Ctrl+Alt+S** ouvre les paramètres : un arbre de pages avec une recherche (Apparence et comportement, Raccourcis clavier, Éditeur, Plugins, Langages et frameworks, Build, exécution, déploiement), un fil d'Ariane, et **OK**, **Annuler** et **Appliquer**. Les pages modifient un brouillon : rien ne change avant Appliquer ou OK, Annuler l'oublie, et **Réinitialiser** rétablit une page.

## Pas encore là

Un débogueur, le commit partiel par blocs, l'historique de défilement du terminal et un marketplace de plugins.
