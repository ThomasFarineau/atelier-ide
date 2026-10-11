# Node.js (npm, pnpm, Yarn, Bun)

Le plugin intégré Node.js ajoute la fenêtre **npm** sur le rail de droite, seulement dans un projet qui a un `package.json`. Elle liste tous les `package.json` du projet (racine, espaces de travail et sous-dossiers, sans `node_modules`), avec leurs scripts dans l'ordre du fichier.

Le gestionnaire de paquets de chaque `package.json` est celui de son champ `packageManager`, sinon celui du fichier de verrouillage de son dossier ou d'un dossier parent (`bun.lock(b)`, `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json`), sinon npm.

- **Double-clic** (ou ▶) sur un script : il s'exécute dans un nouveau terminal, depuis le dossier de son `package.json` (`pnpm run build`…). Sous Windows, les lanceurs `.cmd` sont utilisés pour que la politique d'exécution de PowerShell ne bloque rien.
- Menu contextuel d'un `package.json` : installer ou mettre à jour les dépendances, voir les paquets obsolètes, ouvrir le fichier. D'un script : l'exécuter ou aller à sa ligne.
- Des dépendances déclarées mais pas installées sont signalées une fois, par une notification.
- Rechercher partout : **Node.js : installer les dépendances**, **Node.js : scripts npm**.

Ce que lance la fenêtre devient une [configuration d'exécution](../guide/running) temporaire, à relancer avec Maj+F10.
