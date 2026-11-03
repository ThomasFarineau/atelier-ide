---
default: minor
---

# Plugin YAML / TOML, et coloration des langages par leur plugin

**YAML / TOML.** Un nouveau plugin colore et reformate (Ctrl+Alt+L) les fichiers YAML et TOML en gardant les commentaires, les guillemets et l'ordre des clés ; un reformatage qui changerait les données du fichier est refusé. Le menu contextuel d'un fichier `.yaml` ou `.yml` de l'arbre du projet propose « Convertir en JSON », qui écrit le JSON à côté du fichier et l'ouvre.

**Coloration.** La coloration d'un langage vient de son plugin (JavaScript, Rust, Java, Markdown, YAML / TOML) : un plugin désactivé laisse ses fichiers en texte brut. Les fichiers TOML, que la coloration ne connaissait pas, sont désormais colorés, et le plugin Cargo colore `Cargo.lock`.
