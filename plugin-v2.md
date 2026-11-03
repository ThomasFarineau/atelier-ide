# Plugins natifs v2 : une interface stable

Fait le 3 novembre 2026 : interface `atelier-plugin-api` 0.3 (version d'ABI **2**), IDE qui ne
charge plus que la v2, trois plugins portés.

## Ce qui a été fait

### L'interface (`api/`, atelier-ide-org/api, tag `v0.3.0`)

L'IDE et un plugin ne partagent que des types `#[repr(C)]` et des fonctions `extern "C"`
(`abi.rs`), plus des messages JSON par nom (`protocol.rs`). Sans fonctionnalité, le crate ne
dépend de rien ; l'IDE n'active que `serde`.

Choix fait au lieu d'une API de widgets dessinés par l'IDE : **le plugin embarque son propre
egui** (fonctionnalité `egui`, de la version qu'il veut). Pour chaque surface (aperçu d'un
fichier, fenêtre d'outils) :

1. l'IDE envoie les entrées de l'image (pointeur, molette, touches et texte quand la surface a le
   clavier, taille de la fenêtre et de la surface, temps) ;
2. le plugin fait tourner son `egui::Context` et rend des maillages (coordonnées de la fenêtre),
   les textures qu'ils utilisent (son atlas de polices, ses images), et les rectangles de ses
   fenêtres et menus flottants ;
3. l'IDE peint la base dans la surface, le flottant au-dessus de lui (où il bloque le pointeur),
   et rappelle le plugin pour ses callbacks OpenGL (`gl.rs`, `glow` du plugin chargé avec le
   `get_proc_address` de l'IDE, `src/plugins/gl.rs`).

Ce que ça a donné par rapport aux questions de la première version de ce document :
- mode immédiat, sans rien à traduire : le plugin garde tout egui (états des widgets, fenêtres,
  listes, `TableBuilder`, `TextEdit`…) ;
- les textures restent au plugin et sont recopiées dans l'IDE (création, mise à jour partielle,
  libération) ;
- l'apparence suit l'IDE : il envoie son `egui::Style` en JSON, ses couleurs (`ui::theme`) et ses
  polices ; `ui.rs` a ses widgets (boutons à icône, menus, lignes, dialogues), et les icônes de
  l'IDE sont rastérisées par l'IDE (`ui::icon`) ; un plugin ajoute les siennes
  (`ui::register_icon`) ;
- même processus (OpenGL partagé), une panique est arrêtée à la frontière et le plugin est marqué
  en échec.

L'évolution se fait par ajouts : chaque structure commence par sa taille, un appel inconnu ne
répond rien ; un changement incompatible demande un nouveau `abi::VERSION`.

### L'IDE

- `src/plugins/native.rs` : chargeur v2 (refuse la v1 avec un message clair), surfaces,
  routage des entrées et du focus, textures, flottants, curseur, presse-papiers, liens.
- La v1 (empreinte, `rust-toolchain` imposé aux plugins) et les kits (`api@kits`, job `kit` de
  `release.yml`) sont retirés. Le manifeste a `"abi": 2` au lieu de `"atelier"`.
- `src/plugins/catalog.rs` et l'onglet **Plugins › Disponibles** : les releases
  `plugins/<id>/vX.Y.Z` de `releases` (la plus récente de chaque plugin, avec son `plugin.json`
  et la bonne version d'ABI), avec Installer / Mettre à jour.
- Database et MS Office Viewer ne sont plus intégrés (MongoDB, Postgres, MySQL sortent des
  dépendances de l'IDE).
- Un bouton pour copier chaque notification (panneau et bulles).
- `examples/` supprimé ; le projet de démonstration des tests est `test/fixtures/demo`.
- Sous-modules : `api/`, `docs/`, `plugins/gltf-viewer`, `plugins/database`,
  `plugins/msoffice-viewer`. Seul `api/` sert à compiler l'IDE (la CI ne récupère que lui).
- Tests de bout en bout (`test/plugins/native/tests.rs`) : la vraie bibliothèque d'un plugin
  compilé à côté est chargée et dessine (aperçu et fenêtre d'outils) sans fenêtre.

### Les plugins

`plugin-gltf-viewer`, `plugin-database`, `plugin-msoffice-viewer` (historique gardé) : chacun se
compile seul avec `atelier-plugin-api` `v0.3.0`. Sa CI vérifie fmt, clippy et tests ; son
**Release** publie `plugins/<id>/vX.Y.Z` (version du plugin) quand la version de `Cargo.toml`
change sur `main` : `<id>.atelier-plugin`, `plugin.json` et `<id>-SHA256SUMS`.

## Ce qui reste

- Créer les dépôts `atelier-ide-org/plugin-database` et `atelier-ide-org/plugin-msoffice-viewer`
  (publics), puis y pousser `main` (les dépôts locaux sont prêts, `origin` déjà réglé) : leur
  release se fait toute seule.
- Fusionner les pull requests de `app` et de `docs` ensemble, puis la pull request de release
  (0.3.0) une fois les trois plugins publiés : sinon les utilisateurs perdent l'explorateur de
  bases et la visionneuse Office le temps qu'ils sortent.
- Supprimer la branche `kits` d'`api`, qui ne sert plus.
- Élargir l'interface au besoin : barre d'état, menus du projet, commandes de fichier, IME,
  glisser-déposer de fichiers, et ce que les prochains plugins demanderont.
