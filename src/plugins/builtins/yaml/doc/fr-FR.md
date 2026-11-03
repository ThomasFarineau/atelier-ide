---
name: YAML / TOML
summary: Coloration et formatage du YAML et du TOML, commentaires gardés, et conversion du YAML en JSON.
---

# YAML et TOML

Le plugin intégré **YAML / TOML** colore les fichiers `.yaml`, `.yml` et `.toml` et les reformate (**Ctrl+Alt+L**). Désactivé dans la fenêtre **Plugins**, ces fichiers s'ouvrent en texte brut.

## Reformater le code

Seule la mise en page change ; les commentaires, les guillemets et l'ordre des clés restent.

- **YAML** : deux espaces par niveau, un espace après `clé:` et après le `-` d'un élément de liste, pas d'espaces en fin de ligne, au plus deux lignes vides de suite. Une liste écrite au niveau de sa clé y reste. Le texte des scalaires de bloc (`|`, `>`) garde sa propre indentation.
- **TOML** : pas d'indentation devant les clés et les tables, `clé = valeur`, `[table]` sans espaces à l'intérieur, deux espaces par niveau dans les tableaux sur plusieurs lignes. Les chaînes sur plusieurs lignes restent telles quelles.

Le fichier est lu avant et après : quand la nouvelle mise en page changerait ses données, ou quand le fichier a une erreur de syntaxe, rien ne change et la barre d'état dit pourquoi.

## Convertir en JSON

Dans l'arbre du projet, le menu contextuel d'un fichier `.yaml` ou `.yml` propose **Convertir en JSON** : le JSON du fichier est écrit à côté (`config.yml` donne `config.json`) puis ouvert. Les ancres, les alias et les clés de fusion (`<<: *defaults`) sont résolus ; un fichier de plusieurs documents (`---`) donne un tableau de ces documents. Un fichier JSON déjà présent n'est jamais écrasé.
