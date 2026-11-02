---
default: minor
---

# Plugins natifs, et la visionneuse 3D devient un plugin à installer

Un plugin externe peut apporter une bibliothèque dynamique (`"library"` dans son manifeste, rangé dans `.atelier/plugins/<plugin>/plugin.json`), qui dessine dans l'IDE comme un plugin intégré et ouvre des fichiers dans un onglet de l'éditeur. Elle n'est chargée qu'une fois le plugin activé dans le projet. Atelier refuse une bibliothèque qui n'a pas été compilée exactement comme lui (même compilateur, mêmes crates) au lieu de la charger. Les plugins implémentent le crate `atelier-plugin-api`.

La visionneuse de modèles 3D (glTF, GLB) n'est plus intégrée : c'est désormais le plugin natif `gltf-viewer`, livré avec chaque release (`gltf-viewer-<os>-<arch>`). Pour continuer à ouvrir les modèles en 3D, extrayez-le dans `.atelier/plugins/gltf-viewer/` du projet, puis activez-le dans la fenêtre Plugins.
