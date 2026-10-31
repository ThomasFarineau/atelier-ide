---
default: minor
---

# CSS, SCSS, Sass et Less

Les fichiers `.css`, `.scss`, `.sass` et `.less` sont colorés (sélecteurs, propriétés, variables, mixins, interpolations) et analysés sans serveur : erreurs de syntaxe, structure, complétion des propriétés, des valeurs, des couleurs, des pseudo-classes, des `@règles`, des variables et des mixins, aller à la déclaration et occurrences des variables, mixins, classes et keyframes. Le serveur de langage de VS Code (« CSS : installer » dans Rechercher partout, avec Node.js) ajoute les erreurs complètes, la documentation et le renommage pour CSS, SCSS et Less.

Une pastille s'affiche devant chaque couleur (`#hex`, `rgb()`, `hsl()`, noms). Un clic ouvre un sélecteur de couleur (teinte, opacité, couleur d'avant et d'après, hexadécimal, canaux, format d'écriture) qui modifie la couleur dans le texte ; un Ctrl+Z annule tous les changements faits avec le sélecteur.

Reformater le code (Ctrl+Alt+L) remet une déclaration et un sélecteur par ligne, même pour un fichier minifié. Le clic droit propose aussi **Minifier** (écrit `nom.min.css`) et **Transformer en SCSS, Less ou Sass** : variables, mixins, `@extend` et interpolations suivent, et les règles CSS qui se suivent sont imbriquées. Le fichier est écrit à côté, sans jamais en écraser un.

Ctrl+/ entoure les lignes CSS de `/* */`.
