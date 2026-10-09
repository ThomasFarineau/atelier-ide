# Atelier IDE

MVP d'IDE desktop écrit en Rust, à l'image d'IntelliJ IDEA (New UI, thème Islands) : barre de titre avec widgets de projet, de branche et d'exécution, rails de fenêtres d'outils, îlots arrondis, onglets, Search Everywhere, notifications et barre d'état avec fil d'Ariane.

## Démarrage

Prérequis : Rust stable et une chaîne de compilation native. Sous Windows, installer les Build Tools Visual Studio avec le composant C++.

```powershell
cargo run
# Ouvrir directement un projet
cargo run -- "C:\chemin\vers\projet"
# Projet de démonstration multilangage
cargo run -- examples/demo
# Produire l'exécutable
cargo build --release
```

L'exécutable Windows est `target/release/atelier-ide.exe`.

## Fonctionnalités

- Interface IntelliJ : vraies icônes « New UI » d'IntelliJ, polices Inter et JetBrains Mono, menu principal (☰), thèmes Islands Dark, Dark et Islands Light, couleurs de coloration syntaxique des schémas IntelliJ Dark/Light.
- Plusieurs projets en même temps : chaque projet s'ouvre dans sa propre fenêtre, comme IntelliJ sous Windows. L'ouverture d'un projet demande « Cette fenêtre » ou « Nouvelle fenêtre ». Le widget de projet liste les projets récents ; un clic droit ouvre directement dans une nouvelle fenêtre.
- Rechercher partout (Double Maj ou Ctrl+P, Ctrl+Maj+N pour les fichiers) : fichiers du projet, onglets ouverts, actions de l'IDE et des plugins, au clavier.
- Notifications en balloons (erreurs, opérations Git) et fenêtre d'outils Notifications sur le rail droit.
- Explorateur de projet avec dossiers pliables, création de fichiers, menu contextuel (copier le chemin, afficher dans l'explorateur, ouvrir dans le terminal, actions Git) et actualisation manuelle. Les dossiers `.git`, `target`, `node_modules`, `.venv` et `__pycache__` sont masqués.
- Éditeur UTF-8 avec onglets, coloration syntaxique, numéros de ligne, undo/redo, recherche exacte, sélection du résultat suivant et indicateur des modifications.
- HTML, CSS, JavaScript, TypeScript, Java, Python et Rust. La coloration TypeScript utilise la grammaire JavaScript pour ce MVP.
- Ouvrir, enregistrer, enregistrer sous et enregistrer tous les fichiers modifiés. Confirmation avant fermeture d'un onglet modifié, changement de projet ou sortie. Une modification externe sur disque bloque l'enregistrement ordinaire pour éviter de l'écraser.
- Paramètres (Ctrl+Alt+S) : apparence, éditeur, raccourcis et plugins. Thème, taille de police, projets récents et activation des plugins sont persistants via le stockage natif eframe.
- Terminal interactif PowerShell sous Windows, `/bin/sh` sous Linux/macOS, via PTY et analyse VT100. Le shell conserve son état (`cd`, variables, processus). Sortie affichée sans couleurs ANSI ; les séquences de positionnement sont interprétées. Zone de saisie pour les commandes ou mode clavier direct pour les programmes interactifs, les flèches et Ctrl+C. Échap rend le clavier à l'IDE.
- Plugins intégrés, compilés dans l'IDE et actifs dans tous les projets, et plugins externes à commandes, activés séparément pour chaque projet. Aucun plugin externe n'est exécuté au chargement.
- Intégration Git complète via le plugin intégré Git (voir ci-dessous).
- Plugin intégré Markdown : pour les fichiers `.md`, trois modes en haut à droite de l'éditeur, comme IntelliJ : éditeur seul, éditeur et aperçu côte à côte (par défaut, défilement synchronisé, séparation redimensionnable), aperçu seul. L'aperçu affiche titres, listes, tableaux, blocs de code colorés, images relatives au fichier et liens ; les cases des listes de tâches se cochent depuis l'aperçu. Les modes sont aussi dans Rechercher partout (« Markdown : … »).
- Glisser-déposer : un fichier s'ouvre dans un onglet, un dossier propose de l'ouvrir comme projet.

## Exécution

F5 enregistre le fichier actif si nécessaire puis envoie une commande au shell du projet. Les outils doivent être installés et accessibles dans le `PATH`.

| Langage | Action |
| --- | --- |
| HTML | Ouvrir le fichier dans le navigateur par défaut |
| CSS | Édition ; ouvrir le HTML associé pour voir le résultat |
| JavaScript | `node chemin/du/fichier.js` |
| TypeScript | `tsx chemin/du/fichier.ts` (installer cet outil séparément) |
| Java | `java chemin/du/Main.java` (JDK avec mode source, Java 11+) |
| Python | `python -u chemin/du/fichier.py` |
| Rust | `cargo run` depuis la racine du projet Cargo |

Une session de terminal démarrée utilise la racine du projet. Après un `cd` manuel, les commandes suivantes utilisent ce nouveau répertoire ; **Redémarrer** revient à la racine du projet.

## Git

Le plugin intégré Git utilise la commande `git` installée (2.26 ou plus récent) et suit le fonctionnement d'IntelliJ :

- **Fenêtre Commit** (Alt+0, rail de gauche) : liste des modifications, des conflits et des fichiers non versionnés, avec cases à cocher. Comme dans IntelliJ, seuls les fichiers cochés sont commités, sans passer par l'index. Options « Modifier le dernier commit » (amend) et Sign-off, Commit et « Commit et pousser… ». Menu contextuel : diff, ouvrir, historique, annuler les modifications, ajouter à Git ou au `.gitignore`, copier le chemin. Onglet Stash : mettre de côté, appliquer, restaurer, supprimer, et voir les fichiers d'un stash.
- **Fenêtre Git** (Alt+9, rail du bas) : log avec graphe des branches, badges des branches et tags, filtres par texte, hash, auteur ou branche, et historique d'un fichier. Le détail d'un commit affiche son message et ses fichiers. Actions : checkout, nouvelle branche, tag, cherry-pick, revert, reset (soft, mixed, hard, keep) et annulation du dernier commit. L'onglet Console liste les commandes git lancées par l'IDE et leur sortie.
- **Widget de branche** dans la barre de titre : branche courante, commits en avance ou en retard, opération en cours. Il donne accès aux branches locales et distantes, au checkout intelligent (stash, checkout puis restauration), à la création, au renommage et à la suppression de branches, au merge et au rebase. Il permet aussi la mise à jour (pull merge ou rebase, avec autostash), le push (commits sortants, force-with-lease, tags), le fetch, le checkout d'un tag ou d'une révision et la gestion des remotes.
- **Diff** côte à côte ou unifié, navigation entre les différences (F7 / Maj+F7) et masquage des lignes identiques.
- **Conflits** : accepter la version locale ou entrante, marquer comme résolu, ou résoudre bloc par bloc dans la fenêtre de résolution. Une fusion, un rebase ou un cherry-pick en cours peut être abandonné ou poursuivi.
- **Éditeur et projet** : marqueurs des lignes ajoutées, modifiées ou supprimées par rapport à HEAD, et annotations Git Blame. Ces actions s'ouvrent par un clic droit sur les numéros de ligne. L'arborescence est colorée selon le statut Git.
- Création d'un dépôt et clonage, depuis le widget ou la palette de commandes.

Les opérations s'exécutent en arrière-plan et le statut se rafraîchit toutes les 5 secondes ou au retour dans la fenêtre. Les fichiers ouverts sont enregistrés avant un commit, un checkout ou une mise à jour, puis rechargés s'ils ont changé sur disque. Le plugin se désactive dans la fenêtre Plugins.

## Intelligence du code (JavaScript)

L'éditeur offre complétion, infobulles et soulignement des erreurs pour JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`), dans `src/languages/`. Le code est organisé pour accueillir d'autres langages : un `Provider` par langage, un `Service` partagé par les éditeurs et un client LSP générique.

- **Complétion** : elle s'ouvre en tapant (après un caractère ou un point) ou avec Ctrl+Espace. Elle propose les variables, fonctions, classes, paramètres et imports visibles, les globaux de JavaScript et de Node.js, les mots clés, des modèles de code (`log`, `fori`, `func`, `try`…), les membres du type de la valeur avant le point (chaînes, tableaux, `Math`, `console`, `Promise`, `Map`, objets littéraux, classes locales, `this`) et les chemins dans `import … from '…'` et `require('…')` (fichiers, dossiers, `node_modules`, dépendances du `package.json`, modules de Node). Flèches pour choisir, Entrée pour insérer, Tab pour remplacer le reste du mot, Échap pour fermer.
- **Infobulle** : le pointeur au repos sur un nom affiche sa déclaration, son type et son commentaire JSDoc (types des `@param` et `@type` compris).
- **Erreurs** : les erreurs de syntaxe sont soulignées en rouge, avec leur nombre dans la barre d'état.
- **Navigation, comme dans IntelliJ** : Ctrl+clic (ou Ctrl+B) sur un nom va à sa déclaration, y compris dans un autre fichier ou un module importé ; sur une déclaration, il liste ses utilisations dans une fenêtre à choisir. Ctrl maintenu sur un nom le souligne comme un lien et montre la ligne de sa déclaration. Ctrl+Shift+B va à la déclaration du type, Ctrl+Alt+B aux implémentations (serveur de langage), Alt+F7 liste les utilisations, Ctrl+F12 ouvre la structure du fichier (fonctions, classes, méthodes) à filtrer en tapant, Ctrl+Alt+Gauche / Droite revient à l'emplacement précédent ou suivant, et Ctrl+Maj+M saute à la parenthèse ou accolade correspondante. Les occurrences du nom sous le curseur sont surlignées, ainsi que la paire de parenthèses ou d'accolades.
- **Actions rapides** : Alt+Entrée sur un nom non déclaré propose de l'importer depuis les autres fichiers du projet (`import { x } from './f.js'` ou `require`, en s'ajoutant à un import existant du même module), depuis un module de Node.js ou depuis une dépendance du `package.json`. Avec le serveur de langage, ce sont ses actions rapides.
- **TypeScript** (`.ts`, `.tsx`, `.mts`, `.cts`) : même moteur que JavaScript, avec la grammaire TypeScript (`src/languages/typescript/`). Les types écrits dans le code sont compris : annotations, interfaces (et `extends`), alias, enums, génériques simples, `Promise<T>` et `await`, tableaux typés (`users.map(u => u.…)`, `for…of`), `as`, membres privés des classes et propriétés de constructeur. La complétion propose les types dans une position de type, les propriétés attendues d'un objet littéral typé, les mots clés et modèles TypeScript ; l'infobulle, la navigation, la structure du fichier et Alt+Entrée (imports sans extension, `@types`) en tiennent compte, et les mots clés TypeScript sont colorés.
- **Recherche et remplacement** : Ctrl+F ouvre la barre de recherche (sélection reprise comme requête, `Aa` casse, `W` mots entiers, `.*` expression régulière — Alt+C, Alt+W, Alt+X —, « 3 sur 12 », occurrences surlignées dans l'éditeur, Entrée / Maj+Entrée ou F3 / Maj+F3 pour avancer) ; Ctrl+R y ajoute le remplacement (« Remplacer » et « Tout remplacer », annulable en une fois par Ctrl+Z). Ctrl+Maj+F ouvre la fenêtre **Rechercher** (`src/plugins/builtins/search/`) : recherche dans tout le projet hors dépendances, avec les mêmes options, un masque de fichiers (`*.js, src/*`), les résultats groupés par fichier et surlignés (clic pour ouvrir à l'endroit), et « Tout remplacer » dans le projet après confirmation.
- **Renommer (Maj+F6)** : un champ s'ouvre sur le nom sous le curseur ; Entrée renomme toutes ses occurrences, y compris dans les fichiers du projet qui l'importent (`import { x }` et ses usages, sans toucher aux alias ni aux homonymes), `{ a }` devient `{ a: b }`, et un nom invalide ou déjà pris est refusé. Avec le serveur de langage, c'est son renommage qui s'applique. Les autres fichiers sont modifiés (ouverts : à enregistrer ; fermés : écrits).
- **Repli du code** : chevrons dans la gouttière (accolades, crochets, parenthèses, commentaires de bloc, groupes d'imports) ; Ctrl+- replie, Ctrl+= déplie, Ctrl+Maj+- / Ctrl+Maj+= tout replier / déplier, Ctrl+. bascule. Un clic sur le `...` déplie, et atteindre une ligne cachée (recherche, navigation) déplie ce qui la cache.
- **Problèmes (Alt+6)** : la fenêtre liste les erreurs et avertissements des documents ouverts, groupés par fichier, avec filtres et accès direct à la ligne ; un clic sur le compteur de la barre d'état l'ouvre. Des marques colorées, le long du bord droit de l'éditeur, montrent où sont les problèmes du fichier et y mènent d'un clic.
- **Serveur de langage** : l'analyse locale (tree-sitter) fonctionne dès l'ouverture d'un fichier. Si `typescript-language-server` est disponible (dans `node_modules/.bin` du projet, dans `~/.atelier/lsp` ou dans le PATH), il répond à sa place avec la précision de TypeScript, y compris pour les fichiers `.ts` et `.tsx`, et l'analyse locale reste le repli. La commande « JavaScript : installer le serveur de langage » de la palette l'installe dans `~/.atelier/lsp` (Node.js requis) sans toucher à l'installation globale ; la barre d'état indique le moteur actif (`tsserver` ou `JS : analyse locale`). Les projets WSL utilisent l'analyse locale.

## Plugins

Les plugins intégrés (Git, Markdown) font partie de l'exécutable et peuvent étendre l'interface : fenêtres d'outils, barre de titre, gouttière de l'éditeur, couleurs de l'arborescence, raccourcis et palette (trait `BuiltinPlugin` dans `src/builtin.rs`). Ils sont activés par défaut pour tous les projets.

Les plugins externes sont des manifestes JSON propres à un projet, limités à des commandes. Copier `examples/plugins/rust-tools.json` dans `.atelier/plugins/rust-tools.json` du projet souhaité, cliquer sur **Plugins**, puis **Recharger** et activer l'extension.

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

Les arguments sont transmis comme des valeurs littérales, avec l'échappement du shell. Les commandes s'exécutent avec les droits de l'utilisateur ; activer uniquement des manifestes de confiance. Cette API fournit des commandes externes, pas des bibliothèques natives ou un marketplace.

## Raccourcis

| Raccourci | Action |
| --- | --- |
| Ctrl+O | Ouvrir un fichier |
| Ctrl+N | Nouveau document |
| Ctrl+S | Enregistrer |
| Ctrl+Shift+S | Enregistrer sous |
| Ctrl+W | Fermer l'onglet |
| Ctrl+F | Rechercher dans le fichier |
| Double Maj / Ctrl+P | Rechercher partout |
| Ctrl+Shift+N | Aller au fichier |
| Ctrl+Alt+S | Paramètres |
| Alt+1 | Fenêtre Projet |
| Ctrl+` | Afficher/masquer le terminal |
| F5 | Exécuter |
| Ctrl+Espace | Complétion du code |
| Alt+Entrée | Actions rapides (importer…) |
| Ctrl+F12 | Structure du fichier |
| Alt+F7 | Utilisations |
| Ctrl+Maj+B / Ctrl+Alt+B | Déclaration du type / implémentations |
| Ctrl+K | Commit |
| Ctrl+Shift+K | Pousser |
| Ctrl+T | Mettre à jour le projet (pull) |
| Alt+0 / Alt+9 | Fenêtres Commit / Git |
| F7 / Maj+F7 | Différence suivante / précédente dans le diff |

Sur macOS, utiliser Cmd pour les raccourcis Ctrl de l'éditeur.

## Validation

```powershell
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

## Périmètre du MVP

L'éditeur permet de gérer des fichiers et d'exécuter les outils du projet. L'autocomplétion sémantique/LSP, le débogueur, les diagnostics inline, le commit partiel par blocs, le rechargement automatique des fichiers et un marketplace de plugins restent à implémenter. La taille maximale d'un fichier est 4 Mo ; les fichiers binaires sont refusés. Le terminal affiche l'écran courant, sans navigation dans l'historique de sortie.

## Mode développement (hot reload)

```powershell
cargo dev            # alias de : cargo run -- --dev
cargo dev -- examples/demo
```

Le processus lancé surveille `src/`, `Cargo.toml` et `Cargo.lock`. À chaque modification, il recompile l'IDE dans `target/hot` ; si la compilation réussit, la fenêtre est remplacée par la nouvelle version en conservant le projet, les onglets, le texte non enregistré, l'onglet actif, les panneaux et la position de la fenêtre. En cas d'erreur de compilation, les messages s'affichent dans la console et l'IDE en cours continue de tourner. Le shell du terminal intégré est redémarré à chaque rechargement. Fermer la fenêtre arrête le mode dev.

## Architecture

- `src/app.rs` : coquille de l'IDE (barre de titre, rails, fenêtres d'outils, onglets, éditeur, barre d'état, dialogues), raccourcis et cycle de vie des documents.
- `src/ui.rs` : look and feel IntelliJ partagé avec les plugins (thèmes, polices, icônes, widgets).
- `src/highlight.rs` : coloration syntaxique aux couleurs d'IntelliJ.
- `src/languages/` : intelligence du code — `mod.rs` (types, `Provider`, `Service`), `lsp/` (client du Language Server Protocol) et `javascript/` (analyse tree-sitter, complétion, infobulles, tables des API de JavaScript et Node.js, serveur TypeScript).
- `src/editor/assist.rs` : popup de complétion, soulignement des erreurs et infobulle de l'éditeur.
- `src/project.rs` : documents, détection des langages, accès aux fichiers.
- `src/terminal.rs` : pseudo-terminal, shell et écran VT100.
- `src/plugins.rs` : plugins externes, manifestes JSON et construction des commandes.
- `src/builtin.rs` : contrat des plugins intégrés (`BuiltinPlugin`, `Host`).
- `src/git/` : plugin Git — `repo.rs` (appels à git et analyse des sorties), `diff.rs`, `conflict.rs`, `plugin.rs` (état et opérations en arrière-plan) et `plugin/views.rs` (interface).
- `src/markdown.rs` : plugin Markdown (modes d'affichage et aperçu).
- `src/dev.rs` : mode développement, recompilation et rechargement à chaud de l'IDE.

Références techniques : [eframe](https://docs.rs/eframe/0.33.3/eframe/) et [portable-pty](https://docs.rs/portable-pty/0.9.0/portable_pty/).

## Installation et mises à jour

Chaque tag `vX.Y.Z` lance le workflow `Release` (`.github/workflows/release.yml`) : il compile l'IDE et son
bootstrap pour Windows, Linux et macOS (Intel et Apple Silicon) et publie une release GitHub.

```bash
# la version de Cargo.toml doit être celle du tag
git tag v0.1.0 && git push origin v0.1.0
```

On ne télécharge qu'une fois le **bootstrap** (`atelier-<os>-<arch>`, `.exe` sur Windows) de la dernière release et on
le lance toujours à la place de l'IDE. À chaque démarrage il :

1. demande à GitHub la dernière release (4 s maximum, sans réseau il passe) ;
2. si elle est plus récente que `app/VERSION` (ou que l'IDE n'est pas encore là), télécharge l'archive de la
   plateforme et la vérifie avec `SHA256SUMS` ;
3. remplace le dossier `app/` d'un bloc, puis lance l'IDE. L'IDE n'étant pas démarré, rien n'est jamais verrouillé
   sous Windows. Une archive incorrecte ne change rien : l'ancienne version démarre.

Le bootstrap se met aussi à jour avec chaque release. Le journal du dernier démarrage est dans `bootstrap.log`.
Variables : `ATELIER_NO_UPDATE=1` saute la vérification, `ATELIER_REPO=propriétaire/dépôt` vise un autre dépôt,
`ATELIER_GITHUB_TOKEN` donne accès à un dépôt privé. Sur macOS, un bootstrap téléchargé par le navigateur est mis en
quarantaine : `xattr -d com.apple.quarantine atelier-macos-aarch64`.

## Ouvrir depuis le terminal

```bash
atelier .              # le dossier devient le projet
atelier src/main.rs    # le fichier s'ouvre dans l'IDE déjà ouvert
atelier -n ~/autre     # force une nouvelle fenêtre
```

Si un IDE est déjà ouvert, la commande lui confie le chemin (celui dont le projet contient le fichier, sinon le dernier
utilisé) et se termine tout de suite ; un dossier passe par le choix « cette fenêtre / nouvelle fenêtre ». Sans IDE
ouvert, elle en lance un. La commande `atelier` est le bootstrap : la première installation l'ajoute au PATH, et
`atelier --install-cli` le refait à la demande. Avec un chemin, la recherche de mise à jour n'a lieu que toutes les six
heures pour que la commande reste instantanée.

## Serveur MCP pour les IA

Le plugin **Atelier Super Intelligence Tools** (le serveur MCP) fait d'Atelier un serveur MCP (HTTP, `127.0.0.1` seulement, protégé par un secret conservé d'une
session à l'autre). Les terminaux de l'IDE définissent `ATELIER_MCP_URL` et `ATELIER_MCP_TOKEN` ; quand on y lance
`claude`, `gemini` ou `codex`, l'IDE retient la touche Entrée et propose de connecter l'IA : « Connecter » écrit le serveur
dans `.mcp.json` (et l'approbation dans `.claude/settings.local.json`) pour Claude Code, dans
`.gemini/settings.json` pour Gemini CLI, ou dans `.codex/config.toml` pour Codex. Aucun fichier ne contient le secret
(`${ATELIER_MCP_TOKEN}` ou `bearer_token_env_var`). Claude et Gemini lisent aussi l'adresse dans `${ATELIER_MCP_URL}`,
donc leurs fichiers restent valables quand le port change ; Codex ne développe pas les variables, son fichier contient
l'adresse du moment et se réécrit quand elle change (le port est conservé d'une session à l'autre, ce qui l'évite le plus
souvent), donc mieux vaut ne pas le versionner. La réponse est retenue par
projet. Les commandes « MCP : connecter… » de la palette font la même chose à la demande.

Outils proposés : `get_project_info`, `get_open_files`, `get_active_editor` (curseur et sélection), `open_file`
(à une ligne), `get_problems` (erreurs et avertissements des fichiers ouverts) et `search_in_project` (hors du fil de
l'interface, la fenêtre ne se fige pas). Un autre client MCP peut s'y connecter avec l'adresse et le secret
(`mcp.json` du dossier de données d'Atelier) : `Authorization: Bearer <secret>`.

### Sessions d'IA

Chaque connexion d'une IA au serveur MCP est une **session du projet**, enregistrée dans `.atelier/sessions/` (le
dossier est ignoré par git : `.atelier/.gitignore`). La fenêtre **Sessions IA** (Alt+7) les liste, avec l'IA, l'heure,
les appels faits à l'IDE et les notes laissées.

Le protocole ne montre pas la conversation, seulement les appels d'outils. Le contexte est donc tenu par l'IDE : l'IA
est invitée (instructions du serveur) à noter l'objectif, les décisions, les tâches restantes et l'avancement avec
`session_note`. **Reprendre avec…** écrit ce contexte dans `.atelier/sessions/<id>.md`, connecte l'IA choisie (Claude
Code, Codex ou Gemini CLI, pas forcément celle qui avait commencé) et la lance dans un nouveau terminal avec une première
demande qui renvoie à ce fichier. Elle peut aussi appeler `get_session_context` (`id` d'une session) et `list_sessions`.
« Copier le contexte » le met dans le presse-papiers pour n'importe quel autre outil.

## Graphe de contexte

Un plan du projet, fait **sans IA** et tenu à jour, que les IA lisent au lieu d'ouvrir des dizaines de fichiers (moins
de contexte consommé). Dans un dépôt dont on n'a pas encore décidé, la fenêtre Projet propose « Générer le graphe de
contexte ? » ; le bouton graphe de son en-tête fait la même chose à tout moment (générer, ouvrir le rapport,
régénérer, désactiver). La réponse est retenue dans `.atelier/config.json`.

Tant que l'option est activée, un fil d'exécution à part regarde les fichiers (à chaque enregistrement et toutes les
10 s), relit seulement ceux qui ont changé et reconstruit le graphe, en tâche d'arrière-plan visible dans la barre
d'état. Sont lus : JavaScript et TypeScript, Rust (fonctions, types, méthodes, imports, modules, appels, héritage) et
les titres et liens des fichiers Markdown. Chaque lien est `EXTRACTED` (lu dans le code) ou `INFERRED` (déduit d'un nom
unique dans le projet) ; les fichiers qui s'appuient les uns sur les autres forment des groupes.

Fichiers dans `.atelier/graph/` : `GRAPH_REPORT.md` (le résumé à lire en premier : éléments centraux, groupes, et une
estimation du pourcentage de tokens économisés ; écrit en anglais, pour les IA),
`graph.json` (format nœuds-liens) et `cache.json` (ce que disait chaque fichier, pour ne rien relire au redémarrage).
Le dossier est ignoré par git (`.atelier/.gitignore`) : le graphe se régénère, il n'y a rien à partager ; supprimer la
ligne pour le versionner. Les IA l'interrogent par les outils MCP `graph_overview`, `graph_query`, `graph_node` et
`graph_path`, dont les réponses s'arrêtent à un budget de tokens.

### Commits par l'IA

Le serveur propose une commande **`commit`** (un « prompt » MCP : `/mcp__atelier__commit` dans Claude Code) avec deux
modes, aussi disponibles seuls en `commit-all` et `commit-by-context` :

- **all** : un seul commit avec tous les changements ;
- **context** : un commit par changement cohérent. Les fichiers qui s'appuient les uns sur les autres (via le graphe de
  contexte, s'il est activé) ou d'un même dossier vont ensemble ; la doc, les dépendances (`Cargo.lock`,
  `package.json`…) et la CI ont chacune leur commit. Au plus 8 commits.

L'IA passe par trois outils : `git_commit_plan` (les conventions du dépôt lues dans son historique, les fichiers et leur
découpage), `git_diff` (ce qui a changé) et `git_commit` (les commits). Les **conventions du dépôt** sont déduites des 60
derniers commits : Conventional Commits, gitmoji ou texte libre, français ou anglais, types et portées utilisés,
majuscule, point final, longueur, présence d'un corps. Chaque message est vérifié avant tout commit : si un seul ne les
respecte pas, rien n'est commité. **Jamais de co-auteur** : les lignes `Co-authored-by` et les attributions à une IA
sont retirées du message, qui qu'en soit l'auteur. Rien n'est poussé ni modifié après coup (pas d'amend).
