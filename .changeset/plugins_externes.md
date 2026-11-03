---
default: major
---

# Le lecteur multimédia devient un plugin, et un onglet Langages

Le lecteur de musique et de vidéos n'est plus intégré : c'est désormais le plugin natif `media-viewer`, à installer depuis **Plugins › Marketplace**, comme `database`, `msoffice-viewer` et `gltf-viewer`. rodio et le téléchargement de ffmpeg sortent des dépendances de l'IDE.

La fenêtre **Plugins** a trois onglets : **Plugins** (intégrés et installés), **Marketplace** (les plugins publiés, avec les fichiers qu'ils ouvrent) et **Langages**. Un langage est automatique par défaut : activé tant que le projet contient un de ses fichiers (un `.rs` pour Rust, un `.py` pour Python…).

Le `plugin.json` d'un plugin natif liste les extensions des fichiers qu'il prend en charge, et leurs icônes sont dans son dossier `extensions/` (`a+b.svg`). `atelier-cli` (dépôt `api`) crée un plugin (`atelier-cli plugin create`) et produit le fichier à installer (`atelier-cli plugin compile`).
