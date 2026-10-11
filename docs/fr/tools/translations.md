# Traductions (i18n)

Le plugin intégré **Traductions** gère les fichiers de traduction d'un projet à partir d'une **source** : la langue de référence, enregistrée dans `.atelier/i18n.json`.

## La source

La source peut être :

- **un fichier** ; les autres langues sont trouvées à côté de lui :
  - `locales/fr.json`, `locales/en_US.json` : un fichier par langue dans un dossier ;
  - `locales/fr/common.json`, `locales/en/common.json` : un dossier par langue ;
  - `messages.properties`, `messages_fr.properties` : bundles Java (le fichier sans suffixe est la langue par défaut) ;
- **le dossier d'une langue** (`locales/fr`) : chaque fichier dedans, sous-dossiers compris, est un espace de noms, et ses clés s'écrivent `espace:clé` (`common:menu.open`, `auth/login:title`), comme avec i18next. Les autres langues sont les dossiers voisins ; un espace de noms qu'une langue n'a pas encore est créé à la première valeur écrite ;
- **une expression régulière** sur les chemins des fichiers du projet (relatifs, avec `/`), avec un groupe `(?<lang>…)` pour la langue et, facultatif, `(?<ns>…)` pour l'espace de noms : `src/i18n/messages\.(?<lang>\w+)\.json`, `app/(?<ns>\w+)/lang/(?<lang>[a-z]{2})\.json`. On précise aussi la langue de référence (par défaut l'anglais s'il est là, sinon la première). Sans `ns`, une langue qui a plusieurs fichiers les distingue par leur chemin.

Les JSON peuvent être plats (`"menu.open"`) ou imbriqués (`{"menu": {"open": …}}`).

## La fenêtre Traductions

En bas. Elle propose d'abord comme source les fichiers et dossiers qui ressemblent à des traductions, le fichier actif, ou une expression régulière. Elle montre ensuite toutes les clés dans toutes les langues :

- manquantes en rouge ;
- vides, ou avec d'autres variables que la source (`{name}`, `{{count}}`, `%s`, `${x}`), en orange ;
- identiques à la source en gris ;
- avec le pourcentage traduit de chaque langue.

Un clic sur une cellule la modifie et l'écrit dans son fichier (à sa place, avec l'indentation du fichier). On peut ajouter une langue (un fichier créé avec toutes les clés de la source) ou une clé (dans toutes les langues) ; le menu d'une clé l'ouvre dans la source ou la supprime partout. Le tableau suit les fichiers modifiés hors de l'IDE.

## Pour les agents IA

L'outil MCP **`i18n`** (voir [Serveur MCP](../ai/mcp)) donne la même chose aux agents IA. `action` vaut :

| Action | Résultat |
| --- | --- |
| `languages` | Les langues |
| `check` | Toutes les langues comparées à la source |
| `check_language` | Une langue en détail (avec `language`) |
| `add_language` | Crée une langue, avec des valeurs vides ou, avec `fill: "source"`, le texte de la source |

Les résultats listent les clés manquantes, en trop, vides, aux variables différentes et identiques à la source. `source` choisit un fichier ou un dossier source, `pattern` (avec `source_language`) une expression régulière.
