---
default: minor
---

# Runtimes du projet et notebooks Jupyter

Le nouveau plugin **Runtimes & Jupyter** installe un Python, des JDK et des Node.js dans le projet (`.atelier/runtimes`), vérifiés par leur SHA-256. Les nouveaux terminaux du projet les ont en tête du `PATH` (avec `JAVA_HOME` et le `.venv` activé). Les versions épinglées par `.python-version`, `.java-version`, `.sdkmanrc`, `.nvmrc` ou `.node-version` sont lues et proposées à l'installation.

Les fichiers `.ipynb` s'ouvrent en notebook : cellules éditées et exécutées sur place (Shift+Entrée) avec le Python du projet, sorties texte, images et Markdown, et ouverture dans JupyterLab.
