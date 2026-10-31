---
default: minor
---

# Python

Sans serveur, Atelier connaît les fichiers Python par lui-même : erreurs de syntaxe, structure (classes, méthodes, fonctions, noms du module), occurrences d'un nom dans sa portée, aller à la déclaration (paramètres, affectations, boucles, imports, `self.x`), complétion des noms du fichier, des membres de `self`, des mots-clés et des fonctions intégrées, et modèles (`main`, `init`, `try`, `prop`…).

pyright (ou basedpyright, pris dans le venv du projet, le `PATH`, ou installé avec « Python : installer » dans Rechercher partout) prend le relais : types, erreurs, documentation, renommage, avec les paquets du `.venv` du projet. Reformater le code passe par ruff, sinon black. Les fichiers `.pyi` et `.pyw` sont aussi du Python.
