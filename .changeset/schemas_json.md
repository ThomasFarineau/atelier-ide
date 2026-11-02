---
default: minor
---

# Schémas JSON, et celui de package.json

Les fichiers JSON signalent leurs erreurs de syntaxe pendant la frappe ; les commentaires et les virgules finales des réglages d'outils (`tsconfig.json`…) ne sont pas des erreurs.

Un fichier JSON peut avoir un schéma : celui que nomme sa clé `$schema` (un fichier du projet, ou l'adresse d'un schéma qu'apporte un plugin), sinon celui qu'un plugin apporte pour son nom. Ses valeurs sont alors vérifiées, ses clés et leurs valeurs complétées, et une clé est décrite sous la souris. Le plugin Node.js apporte le schéma de `package.json`.
