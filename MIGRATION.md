# Migration vers l'organisation `atelier-ide-org`

## État au 2 novembre 2026

Tous les dépôts de l'organisation sont **publics**.

| Dépôt | Contenu |
| --- | --- |
| `app` | Le code de l'IDE et du lanceur, le workspace Cargo, `CHANGELOG.md`, `.changeset/`, la CI de build |
| `releases` | Les téléchargements seulement : l'IDE sous `app/vX.Y.Z`, chaque plugin sous `plugins/<nom>/vX.Y.Z` |
| `api` | Le crate `atelier-plugin-api` (branche `main`, un tag par version), et sur la branche `kits` le kit de compilation de chaque version de l'IDE (`vX.Y.Z/`) |
| `plugin-<nom>` | Un plugin natif (`plugin-gltf-viewer`) : son code, son `plugin.json`, ses `locales/` et son `doc/` |
| `docs` | Le site VitePress, publié sur <https://atelier-ide-org.github.io/docs/> |

**Un seul secret** : `ATELIER_RELEASES`, un jeton de l'organisation qui écrit le contenu de
`releases` et d'`api`.
- `app` s'en sert pour publier l'IDE dans `releases` et son kit dans `api`.
- Les plugins s'en servent pour publier dans `releases`.
- Tout le reste se lit sans jeton.

**Comment sort une version :**
1. La release d'`app` publie `app/vX.Y.Z` dans `releases`. Elle publie aussi le kit de
   compilation (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, sans code) dans
   `api@kits/vX.Y.Z/`.
2. La CI de chaque plugin (toutes les six heures, ou à la main) trouve cette release, fait du kit
   un paquet vide, y place le plugin et le compile comme l'IDE. Elle publie ensuite
   `plugins/<nom>/vX.Y.Z` avec un seul fichier, `<nom>.atelier-plugin`, qui contient les 4 systèmes.
3. L'utilisateur l'installe dans l'IDE : Plugins → **+** (« Installer un plugin depuis un
   fichier »). L'installation vaut pour tous les projets.

**Vérifié :** un plugin compilé avec seulement le kit se charge dans l'IDE compilé seul. Même
compilateur et même empreinte, parce que l'API ne dépend que d'eframe.

`app` a été privé quelques heures le 2 novembre, puis repassé en public. Les anciens lanceurs
(≤ `0.1.4`), qui interrogent `ThomasFarineau/atelier-ide`, suivent la redirection vers `app` et
fonctionnent. La release `0.2.0` (publiée à la fois dans `app` et dans `releases`) leur apporte
le lanceur qui lit `releases`.

Ce qui suit est le plan d'origine, gardé pour l'historique. Certains choix ont changé depuis :
- les dépôts sont publics ;
- un seul secret ;
- les plugins ne sont ni des sous-modules ni compilés avec le code d'`app` ;
- `plugin-api` a été renommé `api`.

---

## Les trois contraintes qui fixent l'ordre

### 1. Le lanceur lit les releases sans jeton

`src/bin/atelier.rs` interroge `https://api.github.com/repos/{REPO}/releases/latest` avec
`REPO = "ThomasFarineau/atelier-ide"`. Ce nom redirige aujourd'hui vers `atelier-ide-org/app`.
**Dès que `app` sera privé, GitHub répondra 404 à tous les lanceurs installés**, qui ne se
mettront plus jamais à jour.

Le lanceur se remplace lui-même quand l'archive d'une release en contient un nouveau
(`atelier.exe.old`). On peut donc le faire pointer ailleurs, à condition de publier une release
« pont » **pendant que `app` est encore public**, puis de laisser le temps aux utilisateurs de la
recevoir.

### 2. Les plugins natifs sont liés à une version précise de l'IDE

Rust n'a pas d'ABI stable. Une DLL n'est chargée que si elle a été compilée par le même `rustc`,
pour la même cible et le même profil, avec les mêmes versions **et fonctionnalités** des crates
partagés (egui, eframe, glow), et de la même manière (`--target` compte). `atelier-plugin-api`
le vérifie et refuse proprement une DLL compilée autrement.

Conséquence : un dépôt `plugin-<nom>` **ne peut pas publier seul une DLL utilisable**. La DLL
de la version X de l'IDE doit sortir **du même `cargo build` que l'IDE X**. Le code d'un plugin
vit donc dans son dépôt, mais il est compilé par la CI de `app` et publié dans `releases` avec
l'IDE.

### 3. La doc lit des fichiers du code

Le catalogue des plugins du site (`docs/.vitepress/catalog.ts`) lit le front matter de
`src/plugins/builtins/*/doc/*.md` et `src/languages/*/doc/*.md`. Ces fichiers resteront dans
`app`, qui sera privé. Le dépôt `docs` devra donc récupérer `app` dans sa CI (avec un jeton en
lecture), ou ces pages devront déménager dans `docs`.

---

## Étapes

### Étape 0 — Préparer l'organisation

- Créer `releases` (public, vide, avec un README qui pointe vers le site).
- Créer un **jeton** qui peut écrire dans `releases` : une GitHub App de l'org (de préférence), ou
  un fine-grained PAT limité à `releases`, permission *Contents: read and write*. Le stocker
  dans les secrets de `app` sous le nom `RELEASES_TOKEN`.
- Dans **Settings → Actions** de l'org : autoriser les workflows à écrire et à créer des pull
  requests (`prepare-release.yml` de knope en a besoin, et c'est désactivé par défaut dans une org).
- En local : `git remote set-url origin git@github.com:atelier-ide-org/app.git`. L'ancienne
  URL redirige encore, mais autant ne pas en dépendre.

### Étape 1 — Publier les exécutables dans `releases`

Dans `app` :

1. `src/bin/atelier.rs` : `const REPO: &str = "atelier-ide-org/releases";`
2. `.github/workflows/build.yml` / `release.yml` : knope continue de gérer la version, le
   changelog et le tag dans `app`. Les archives, en revanche, partent dans `releases`, par
   exemple :

   ```yaml
   - run: gh release create "$TAG" artifacts/* --repo atelier-ide-org/releases --title "$TAG" --notes-file notes.md
     env:
       GH_TOKEN: ${{ secrets.RELEASES_TOKEN }}
   ```

   Garder la règle actuelle de knope : la release n'est publiée qu'une fois **tous** les fichiers
   envoyés. Le lanceur ne doit jamais voir une release incomplète. Le plus simple est de la créer
   en brouillon (`--draft`), puis de la publier à la fin.
3. Remplacer l'ancien nom partout où il désigne les téléchargements :

   | Fichier | Nouvelle valeur |
   | --- | --- |
   | `src/bin/atelier.rs` | `REPO = "atelier-ide-org/releases"` |
   | `docs/.vitepress/theme/DownloadButton.vue` | `repo = 'atelier-ide-org/releases'` |
   | `README.md`, `docs/guide/getting-started.md`, `docs/fr/guide/getting-started.md` | liens « dernière release » vers `releases` |
   | `installer/atelier.iss` | `AppPublisherURL` (**ne pas toucher à `AppId`**, sinon Windows ne met plus à jour l'ancienne installation) |
   | `knope.toml` | `[github] owner = "atelier-ide-org"`, `repo = "app"` |
   | `Cargo.toml` | `repository` (privé : on peut le pointer vers `releases` ou vers le site) |

   Pour vérifier qu'il n'en reste aucun :
   `git grep -n "ThomasFarineau" -- . ':!Cargo.lock' ':!CHANGELOG.md'`.
4. **Tester avant de publier.** Le lanceur lit `ATELIER_REPO` : publier une release de test dans
   `releases`, puis lancer `ATELIER_REPO=atelier-ide-org/releases atelier` sur une installation
   existante. Le chemin « API → archive → `SHA256SUMS` » doit fonctionner sans jeton.

### Étape 2 — La release pont

Publier la version suivante **à deux endroits** : dans `app`, encore public, comme d'habitude, et
dans `releases`. Son archive contient le lanceur qui pointe vers `releases`.

- Les lanceurs installés suivent la redirection `ThomasFarineau/atelier-ide` → `app`, y
  trouvent la release pont, s'y mettent à jour et remplacent leur `atelier.exe`.
- À partir de là, ils interrogent `releases` directement.

### Étape 3 — Attendre

Laisser à la release pont le temps de se diffuser : quelques semaines, selon le nombre
d'utilisateurs. Un utilisateur qui n'a pas lancé Atelier pendant cette période restera bloqué sur
l'ancien lanceur après l'étape 4. Il devra retélécharger l'installeur depuis `releases`. Le dire
dans les notes de version.

Pour contrôler : les compteurs de téléchargement des archives de la release pont dans
`releases`, et l'absence de nouveaux téléchargements des anciennes archives dans `app`.

### Étape 4 — Passer `app` en privé

- **Settings → Danger zone → Change visibility.**
- Les anciennes releases de `app` deviennent invisibles. Si on veut garder l'historique public,
  republier les plus récentes dans `releases` avant cette étape.
- Les workflows de `app` consomment alors les minutes Actions de l'org (un dépôt privé n'est plus
  gratuit sans limite). Surveiller surtout la matrice macOS de `build.yml`, la plus coûteuse.
- Ne **jamais** recréer de dépôt `ThomasFarineau/atelier-ide` : cela casserait la redirection
  dont dépendent les retardataires.

### Étape 5 — Sortir la doc dans `docs` ✅

1. Déplacer `docs/` de `app` vers la racine du dépôt `docs` (avec `git filter-repo
   --subdirectory-filter docs` pour garder l'historique). VitePress exclut `app/**` des pages.
2. Le site passe de `atelier-ide-org.github.io/app/` (son adresse depuis le transfert) à
   `atelier-ide-org.github.io/docs/`. Dans `config.mts`, changer `base: '/app/'` en `base: '/docs/'`. GitHub Pages **ne
   redirige pas** d'un propriétaire à l'autre : l'ancienne adresse donnera une 404. Un domaine à
   soi (`CNAME`) évite de refaire ce changement la prochaine fois.
3. Le catalogue des plugins : le workflow de `docs` récupère `app` avec un jeton en lecture seule
   (`READ_TOKEN`) et `actions/checkout` (`repository: atelier-ide-org/app`, `path: app`), puis
   chaque plugin natif dans `app/plugins/<nom>`. `catalog.ts`
   lit `app/src/...` et `app/plugins/*/doc/` (variable `ATELIER_APP`, sinon `../app`). Le site
   est reconstruit chaque nuit pour suivre le code. On garde ainsi la règle actuelle : la page d'un plugin vit à côté de son
   code.
4. Le lien « Edit this page » de `config.mts` ne peut plus viser `app`, qui est privé. Le faire
   pointer vers `docs` pour les pages du site, et le retirer pour les pages qui viennent du code.
5. Retirer `docs/` et `.github/workflows/docs.yml` de `app`.

### Étape 6 — Les plugins dans `plugin-<nom>` ✅

1. **`plugin-api`** : sorti de `plugins/atelier-plugin-api` dans son dépôt public, avec un tag
   par version. `app` et les plugins en dépendent par le **même tag** (Cargo n'en fait alors
   qu'un seul crate, et les `TypeId` sont identiques) :

   ```toml
   atelier-plugin-api = { git = "https://github.com/atelier-ide-org/api", tag = "v0.2.0" }
   ```

   Ce crate ne dépend **que d'eframe**. La v0.1.0 dépendait aussi de `serde_json`. Or l'identité
   d'un crate inclut celle de ses dépendances, et le plugin glTF active une fonctionnalité de
   `serde_json` (via `gltf`) : la DLL ne se chargeait plus dans l'IDE compilé sans elle.
   Vérifié : avec la v0.2.0, un plugin compilé avec l'IDE se charge dans l'IDE compilé seul.
2. **`plugin-gltf-viewer`** : sorti avec `git filter-repo`, avec l'historique de l'ancien plugin
   intégré.
3. **La release de `app` ne dépend d'aucun plugin.** `app` ne contient pas les plugins :
   `members = [".", "plugins/*"]`, `default-members = ["."]`, et `plugins/` est ignoré par git.
   Un plugin cloné dans `plugins/<nom>` rejoint le workspace (`cargo build --workspace`). Le
   compilateur est fixé par `rust-toolchain.toml`, puisque l'empreinte en dépend.

   On avait d'abord mis les plugins en sous-modules de `app`. On les a retirés : la CI et les
   releases de l'IDE dépendaient alors de chaque plugin, et d'un jeton pour lire les plugins
   privés.
4. **Le build des plugins** : le workflow **Release** de chaque `plugin-<nom>` (toutes les six
   heures, ou à la main pour un tag) prend `app` au tag de la dernière release, y place le plugin,
   lance la même commande que `build.yml` (avec `--workspace`), puis publie dans `releases` la
   release `plugins/<plugin>/vX.Y.Z` (ses archives `<plugin>-<os>-<arch>` et
   `<plugin>-SHA256SUMS`), jamais marquée comme la dernière.

   Dans `releases`, l'IDE est publié sous `app/vX.Y.Z` et chaque plugin sous
   `plugins/<nom>/vX.Y.Z` (X.Y.Z : la version de l'IDE). Le lanceur ne lit plus
   `releases/latest`, qui pourrait être un plugin : il prend la plus haute release dont le tag
   commence par `app/` (ou sans préfixe, comme l'ancien `v0.1.4`, renommé `app/v0.1.4`). Secrets de
   l'organisation : `READ_TOKEN` (lit `app`) et `RELEASES_TOKEN` (écrit dans `releases`).
5. **À garder en phase** : la commande de build de `build.yml` dans `app` et celle du workflow
   Release des plugins (cible, profil, pas de `RUSTFLAGS`).
6. **Plus tard** : que l'IDE installe lui-même un plugin depuis **sa propre** release (la seule
   compatible), au lieu de le copier à la main dans `.atelier/plugins/`.

> Pour qu'un plugin puisse un jour publier ses propres DLL, indépendamment de l'IDE, il faudra
> remplacer l'interface Rust par une vraie ABI C (types `#[repr(C)]`, sans egui à travers la
> frontière), ou faire tourner les plugins dans un processus à part. C'est un chantier à part
> entière, pas un prérequis de cette migration.

---

## Calendrier

| # | Étape | `app` | Version |
| --- | --- | --- | --- |
| 0 | Dépôt `releases` ✅, jeton, réglages de l'org | public | — |
| 1 | CI qui publie dans `releases` ✅, `REPO` changé ✅, test avec `ATELIER_REPO` | public | — |
| 2 | Release pont, publiée dans `app` et dans `releases` | public | `0.2.0` |
| 3 | Attendre la diffusion | public | — |
| 4 | `app` passe en privé | **privé** | — |
| 5 | Dépôt `docs` ✅, nouvelle adresse du site ✅ | public | — |
| 6 | `plugin-api` ✅, `plugin-gltf-viewer` ✅, build des plugins dans la CI de `app` ✅ | public | `0.2.0` |

Les étapes 5 et 6 ne dépendent pas l'une de l'autre et peuvent être faites dans l'ordre inverse.
L'étape 6 peut aussi commencer avant l'étape 4, tant que `plugin-api` est public.
