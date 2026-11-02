# Migration vers l'organisation `atelier-ide-org`

## La cible

| Dépôt | Visibilité | Contenu |
| --- | --- | --- |
| `app` | **privé** | Le code de l'IDE et du lanceur, le workspace Cargo, `CHANGELOG.md`, `.changeset/`, la CI de build |
| `releases` *(nom à choisir)* | public | Les exécutables seulement : releases GitHub avec les archives, les installeurs, les plugins compilés et `SHA256SUMS`. Un README, pas de code |
| `plugin-<nom>` | public ou privé | Le code d'un plugin natif (`plugin-gltf-viewer`), avec son `plugin.json`, ses `locales/` et son `doc/` |
| `plugin-api` | public | `atelier-plugin-api`, l'interface que les plugins implémentent |
| `docs` | public | Le site VitePress (aujourd'hui `docs/` de `app`), publié sur GitHub Pages |
| `.github` | public | Optionnel : le profil de l'organisation (`profile/README.md`) |

État au 2 novembre 2026 : `ThomasFarineau/atelier-ide` a été transféré vers
`atelier-ide-org/app`, qui est **encore public**. **Ne pas le passer en privé avant l'étape 4.**

### Où on en est

| Fait | Reste à faire, par toi (droits que Claude n'a pas) |
| --- | --- |
| Dépôt public `atelier-ide-org/releases` créé, avec son README | Créer le jeton `RELEASES_TOKEN` (fine-grained, *Contents: read and write* sur `releases` seulement) et l'ajouter aux secrets de `app` |
| Lanceur (`REPO`), README, doc, installeur, `knope.toml`, `Cargo.toml` pointent vers l'org | Org → Settings → Actions → General : cocher « Allow GitHub Actions to create and approve pull requests » |
| `build.yml` compile et empaquette les plugins, `release.yml` publie dans `releases` (brouillon, puis publication) et dans `app` | En local : `git remote set-url origin git@github.com:atelier-ide-org/app.git` |
| Base du site corrigée en `/app/` (Pages est déjà à `atelier-ide-org.github.io/app/` depuis le transfert), puis le site déplacé dans `docs` | Étapes 2 à 4 ci-dessous |
| Changesets : plugins natifs (`minor`), déménagement des téléchargements (`patch`) | |
| Jeton `RELEASES_TOKEN`, réglage Actions de l'org (fait par toi) | Avant l'étape 4 : un jeton en lecture sur `app` (*Contents: read*), dans les secrets de `docs` sous le nom `APP_READ_TOKEN` |
| `plugin-api` (public, tag `v0.1.0`), `plugin-gltf-viewer` (tag `v0.1.0`), `docs` : historiques extraits avec `git filter-repo` | Pages du dépôt `docs` : Settings → Pages → Source : GitHub Actions |
| `app` : le plugin en sous-module, l'API par son tag, `docs/` retiré | |

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
   (`APP_READ_TOKEN`) et `actions/checkout` (`repository: atelier-ide-org/app`, `path: app`,
   `submodules: recursive`) : les plugins natifs arrivent avec, en sous-modules. `catalog.ts`
   lit `app/src/...` et `app/plugins/*/doc/` (variable `ATELIER_APP`, sinon `../app`). Le site
   est reconstruit chaque nuit pour suivre le code. On garde ainsi la règle actuelle : la page d'un plugin vit à côté de son
   code.
4. Le lien « Edit this page » de `config.mts` ne peut plus viser `app`, qui est privé. Le faire
   pointer vers `docs` pour les pages du site, et le retirer pour les pages qui viennent du code.
5. Retirer `docs/` et `.github/workflows/docs.yml` de `app`.

### Étape 6 — Les plugins dans `plugin-<nom>` ✅

1. **`plugin-api`** : sortir `plugins/atelier-plugin-api` dans son dépôt public, avec un tag par
   version (`v0.1.0`). `app` et les plugins en dépendent par ce tag :

   ```toml
   atelier-plugin-api = { git = "https://github.com/atelier-ide-org/plugin-api", tag = "v0.1.0" }
   ```

   Même URL et même tag : Cargo n'en fait qu'un seul crate. C'est indispensable pour que les
   `TypeId` soient identiques.
2. **`plugin-gltf-viewer`** : sortir `plugins/gltf-viewer` avec `git filter-repo
   --subdirectory-filter plugins/gltf-viewer` (l'historique de l'ancien plugin intégré suit,
   grâce aux `git mv`). Le dépôt garde une CI de tests (`cargo test`), mais **ne publie pas
   de DLL** (contrainte 2).
3. **Dans `app`**, chaque plugin est un **sous-module git** dans `plugins/<nom>`, épinglé sur un
   tag (`v0.1.0`), et un membre du workspace (`members` et `default-members`). On a préféré les
   sous-modules au fichier `plugins.toml` et à son script, prévus au départ : ils épinglent aussi
   une version, mais sans outillage à maintenir, et `Cargo.lock` reste stable. La CI fait
   `actions/checkout` avec `submodules: recursive`. En local :
   `git clone --recursive`, ou `git submodule update --init`.

   Pour livrer une nouvelle version d'un plugin : la taguer dans `plugin-<nom>`, puis
   `cd plugins/<nom> && git checkout vX.Y.Z` dans `app`, et commiter le sous-module.
4. **Le build** : dans `build.yml`, retirer `--bins` de
   `cargo build --release --locked --bins --target …`, sinon les `cdylib` ne sont pas produites.
   Garder **une seule commande** pour l'IDE et les plugins : les fonctionnalités des crates sont
   unifiées, et les empreintes sont identiques.
5. **La publication** : une archive par plugin et par plateforme dans la release de `releases`
   (`plugin-gltf-viewer-windows-x86_64.zip`, avec `plugin.json` et la bibliothèque), ajoutée à
   `SHA256SUMS`.
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
