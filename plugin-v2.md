# Plugins natifs v2 : une API stable, sans egui

Idée à étudier. Rien n'est commencé.

## Le problème de la v1

Un plugin natif v1 est une bibliothèque Rust qui partage les types d'egui avec l'IDE (`egui::Ui`,
`egui::Context`, les callbacks d'`egui_glow`). Rust n'a pas d'ABI stable, ce qui impose trois
contraintes :

- **Le plugin doit être compilé exactement comme l'IDE** : même `rustc`
  (`rust-toolchain.toml`), même cible et même profil, mêmes versions et fonctionnalités des crates
  partagés. L'IDE le vérifie avec une empreinte des `TypeId` partagés (`atelier-plugin-api`) et
  refuse tout le reste.
- **Chaque plugin doit être recompilé pour chaque version de l'IDE.** Une release de l'IDE
  publie son kit de compilation (`api@kits/vX.Y.Z/` : `Cargo.toml`, `Cargo.lock`,
  `rust-toolchain.toml`), puis la CI de chaque plugin recompile tout et publie
  `plugins/<nom>/vX.Y.Z`.
- **Chaque build de plugin compile toutes les dépendances de l'IDE**, soit environ 700 crates
  (MongoDB, tree-sitter, syntect…), uniquement pour reproduire les fonctionnalités des crates
  partagés. Un kit réduit (eframe et ses dépendances, environ 135 crates) réduirait le coût sans
  le supprimer.

La v1 a aussi appris deux choses :
- **L'API ne doit dépendre que d'eframe.** Une dépendance à `serde_json` changeait l'identité
  du crate dès qu'un plugin activait une fonctionnalité de `serde_json` (`gltf` active
  `raw_value`).
- **L'IDE ne doit jamais dépendre d'un plugin.**

## L'idée

Une API qui **ne dépend de rien** : des types `#[repr(C)]`, des pointeurs de fonctions
`extern "C"`, un numéro de version d'ABI. Le plugin ne voit jamais egui : l'IDE lui transmet ce
qui se passe, le plugin répond par ce qu'il faut dessiner, et l'IDE le traduit en egui.

```
IDE (egui)  ──── événements, zone, thème, langue ────▶  plugin
            ◀─── ordres de dessin, widgets, GL ───────
```

### Ce que le plugin reçoit à chaque image

- la zone qui lui est donnée (rectangle, échelle de l'écran) ;
- les événements : pointeur (position, boutons, glisser, double clic), molette, clavier
  (touches et modificateurs), focus, temps écoulé (`dt`) ;
- le thème (couleurs), la langue de l'interface, le fichier ouvert.

### Ce qu'il peut demander à l'IDE (une table de fonctions, l'« hôte »)

- **Dessin** : rectangles (pleins, bordés, arrondis), lignes, texte (taille, couleur,
  alignement), images (pixels RGBA donnés par le plugin, gardés par l'IDE sous un identifiant).
- **Widgets dessinés par l'IDE**, pour garder son apparence : bouton, bouton à icône (avec les
  icônes de l'IDE), case à cocher, curseur, liste déroulante, étiquette, infobulle. Chacun
  renvoie son état : cliqué, nouvelle valeur.
- **Mise en page minimale** : rangée de gauche à droite et de droite à gauche, espacement. Pas
  tout egui, seulement de quoi faire une barre d'outils.
- **OpenGL** : un callback de dessin dans un rectangle, appelé avec le contexte GL courant et
  une fonction `get_proc_address`. Le plugin crée sa propre instance de `glow` à partir de
  cette fonction : le contexte OpenGL est partagé au niveau du système, pas des types Rust.
- **Divers** : demander une nouvelle image (animation), journaliser, notifier.

### Ce que l'IDE appelle dans le plugin

Les mêmes points qu'aujourd'hui, en `extern "C"` :
- `opens_binary`, `editor_layout`, `layout_switch`, `file_icon` (SVG en octets) ;
- `frame` (la vue d'un fichier, à chaque image) ;
- `file_closed`, `destroy`.

Un symbole exporté (`ATELIER_PLUGIN_V2`) donne la version d'ABI et la table de ces fonctions.

### Mémoire et chaînes

Rien ne traverse la frontière en type Rust : pas de `String`, de `Vec` ni de `Box`.
- Les chaînes passent en pointeur + longueur (UTF-8), valables le temps de l'appel.
- Ce que l'un alloue, c'est lui qui le libère (par exemple une fonction `free` dans la table du
  plugin).
- Une panique ne doit jamais traverser la frontière : le plugin l'attrape (`catch_unwind`) et
  renvoie une erreur.

## Ce qu'on y gagne

- **Le plugin ne compile que l'API et ses propres crates.** Pour la visionneuse : `api`,
  `gltf`, `glam` et `glow`, en quelques minutes au lieu de 700 crates.
- **Un plugin marche avec toutes les versions de l'IDE qui gardent la même version d'ABI.**
  Plus de kit, plus de rebuild à chaque release, plus de `rust-toolchain.toml` imposé. Une
  release de plugin devient indépendante de l'IDE (`plugins/<nom>/vX.Y.Z` porterait la
  version du plugin).
- **Plus d'empreinte à vérifier**, seulement le numéro d'ABI.
- **Un plugin pourrait même être écrit dans un autre langage** (C, Zig…), puisque c'est une
  ABI C.

## Ce que ça coûte

- **Concevoir et maintenir l'API** : dessin, widgets, événements, GL. Côté IDE, la traduction
  en egui.
- **Réécrire la visionneuse glTF contre cette API**, surtout sa barre de contrôles (lecture,
  curseur de temps, vitesse, boucle, choix de l'animation, grille). Le rendu OpenGL lui-même
  change peu (`glow` du plugin au lieu de celui d'eframe).
- **Une ABI publiée n'évolue que par ajouts** : nouvelles fonctions en fin de table, ou
  nouvelle version d'ABI. Sinon, les plugins existants cassent.
- **Un plugin ne peut faire que ce que l'API expose.** Il n'a plus accès à egui en entier.

## Comment y aller

1. **Une première version de l'API limitée à ce dont la visionneuse a besoin** : zone et
   événements, rectangles et texte, bouton à icône, case, curseur, liste déroulante, callback
   OpenGL, demande de nouvelle image.
2. **L'IDE charge les deux versions** : un plugin v1 (`ATELIER_PLUGIN`, empreinte) ou v2
   (`ATELIER_PLUGIN_V2`, numéro d'ABI). La v1 continue de marcher pendant la transition.
3. **Porter la visionneuse en v2.** Sa CI se simplifie : un `cargo build` du plugin seul, une
   release qui ne dépend plus de la version de l'IDE.
4. **Retirer la v1 et les kits** une fois les plugins portés.
5. **Élargir l'API plugin par plugin**, selon les besoins réels.

## Questions ouvertes

- Faut-il un mode immédiat (le plugin redemande ses widgets à chaque image, comme egui), ou
  une description retenue ? Le mode immédiat est le plus proche d'egui, donc le plus simple à
  traduire.
- Comment le plugin garde-t-il l'état d'un widget (curseur tenu, liste ouverte) ? Avec des
  identifiants fournis par le plugin, que l'IDE passe à egui (`Id`).
- Les textures : qui les garde, et combien de temps ? Probablement l'IDE, avec une libération
  explicite.
- Faut-il aller jusqu'à un plugin dans un processus à part (crash isolé, pas de `unsafe` dans
  l'IDE) ? C'est plus robuste, mais l'OpenGL partagé devient impossible.
