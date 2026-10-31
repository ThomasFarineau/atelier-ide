---
default: minor
---

# Plugin Cargo, et Jupyter séparé de Runtimes

**Cargo.** Une fenêtre Cargo, sur le rail de droite des projets qui ont un `Cargo.toml`, liste les paquets et les workspaces du projet avec leurs cibles (binaires, exemples, tests, benchmarks). Un double-clic exécute une cible ; le menu d'un `Cargo.toml` compile, vérifie, teste, lance Clippy, formate, ouvre la documentation, met à jour les dépendances ou nettoie.

**Jupyter.** Les notebooks ont leur propre plugin, Jupyter, qui se désactive sans toucher aux environnements du projet ; le plugin Runtimes ne s'occupe plus que des Python, JDK et Node.js du projet et de son `.venv`.
