# JavaScript et TypeScript

L'éditeur offre complétion, infobulles et soulignement des erreurs pour JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`) et TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`). Une analyse locale avec tree-sitter fonctionne dès l'ouverture d'un fichier ; quand `typescript-language-server` est disponible, il répond à sa place, avec la précision de TypeScript.

## Complétion

Elle s'ouvre en tapant (après un caractère ou un point) ou avec **Ctrl+Espace**, et propose :

- les variables, fonctions, classes, paramètres et imports visibles ;
- les globaux de JavaScript et de Node.js, les mots clés et des modèles de code (`log`, `fori`, `func`, `try`…) ;
- les membres du type de la valeur avant le point : chaînes, tableaux, `Math`, `console`, `Promise`, `Map`, objets littéraux, classes locales, `this` ;
- les chemins dans `import … from '…'` et `require('…')` : fichiers, dossiers, `node_modules`, dépendances du `package.json`, modules de Node.

Flèches pour choisir, **Entrée** pour insérer, **Tab** pour remplacer le reste du mot, **Échap** pour fermer.

## Infobulles et erreurs

- Le pointeur au repos sur un nom affiche sa déclaration, son type et son commentaire JSDoc (types des `@param` et `@type` compris).
- Les erreurs de syntaxe sont soulignées en rouge et comptées dans la barre d'état.

## Navigation

- **Ctrl+clic** (ou **Ctrl+B**) sur un nom va à sa déclaration, y compris dans un autre fichier ou un module importé ; sur une déclaration, il liste ses utilisations dans une fenêtre.
- **Ctrl** maintenu sur un nom le souligne comme un lien et montre la ligne de sa déclaration.
- **Ctrl+Maj+B** va à la déclaration du type, **Ctrl+Alt+B** aux implémentations (serveur de langage), **Alt+F7** liste les utilisations.
- **Ctrl+F12** ouvre la structure du fichier (fonctions, classes, méthodes), filtrée en tapant.
- **Ctrl+Alt+Gauche / Droite** revient à l'emplacement précédent ou suivant, **Ctrl+Maj+M** saute à la parenthèse correspondante.
- Les occurrences du nom sous le curseur sont surlignées, ainsi que la paire de parenthèses.

## Actions rapides et renommage

- **Alt+Entrée** sur un nom non déclaré propose de l'importer depuis les autres fichiers du projet (`import { x } from './f.js'` ou `require`, en rejoignant un import existant du même module), depuis un module de Node ou depuis une dépendance du `package.json`. Avec le serveur de langage, ce sont ses actions rapides.
- **Maj+F6** renomme partout le nom sous le curseur, y compris dans les fichiers du projet qui l'importent (`import { x }` et ses usages, sans toucher aux alias ni aux homonymes) ; `{ a }` devient `{ a: b }`, et un nom invalide ou déjà pris est refusé. Les autres fichiers sont aussi modifiés : ouverts, ils sont à enregistrer ; fermés, ils sont écrits.

## TypeScript

Même moteur, avec la grammaire TypeScript. Les types écrits dans le code sont compris : annotations, interfaces (et `extends`), alias, enums, génériques simples, `Promise<T>` et `await`, tableaux typés (`users.map(u => u.…)`, `for…of`), `as`, membres privés des classes et propriétés de constructeur.

La complétion propose les types dans une position de type, les propriétés attendues d'un objet littéral typé, et les mots clés et modèles de TypeScript ; l'infobulle, la navigation, la structure du fichier et Alt+Entrée (imports sans extension, `@types`) tiennent compte des types, et les mots clés de TypeScript sont colorés.

## Serveur de langage

Quand `typescript-language-server` est trouvé (dans `node_modules/.bin` du projet, dans `~/.atelier/lsp` ou dans le PATH), il prend le relais, aussi pour les fichiers `.ts` et `.tsx` ; l'analyse locale reste le repli. **JavaScript : installer le serveur de langage**, dans Rechercher partout, l'installe dans `~/.atelier/lsp` (Node.js requis), sans toucher à l'installation globale. La barre d'état indique le moteur actif (`tsserver` ou `JS : analyse locale`). Les projets WSL utilisent l'analyse locale.
