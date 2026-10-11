# Reformater le code

Comme dans IntelliJ :

| Raccourci | Action |
| --- | --- |
| Ctrl+Alt+L | Reformate le fichier actif, ou seulement les lignes sélectionnées (Code › Reformater le code) |
| Ctrl+Alt+Maj+L | Ouvre **Reformater le fichier** : tout le fichier, le texte sélectionné, ou seulement le texte modifié depuis le dernier commit (d'après les marques de Git), avec l'option **Optimiser les imports** |
| Ctrl+Alt+O | Optimise seulement les imports |

La barre d'état indique combien de lignes ont changé, ou que le code était déjà bien formaté. Le nouveau texte arrive par petites modifications : le curseur reste sur le même code, et un seul **Ctrl+Z** annule tout.

## Selon le langage

| Langage | Formateur |
| --- | --- |
| Rust | rustfmt, par rust-analyzer quand il tourne, sinon directement, avec l'édition de la crate et le `rustfmt.toml` du projet. Les fins de ligne du fichier sont gardées. |
| JavaScript, TypeScript | Le formateur de tsserver (serveur de langage), avec l'indentation du fichier. L'optimisation des imports les trie et retire ceux qui ne servent pas. |
| Java | jdtls |
| JSON | Atelier lui-même, avec l'indentation du fichier et l'ordre des clés |
