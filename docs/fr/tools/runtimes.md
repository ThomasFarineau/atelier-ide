# Runtimes

Le plugin intégré **Runtimes** installe un Python, des JDK et des versions de Node.js **dans le projet lui-même**. Sa fenêtre **Runtimes** est sur le rail de droite ; les plugins [Jupyter](./jupyter) et [Node.js](./node) utilisent ce qu'il installe.

## Les environnements du projet

Chaque environnement est téléchargé dans `.atelier/runtimes/<python|jdk|node>/<version>`, vérifié par son SHA-256 et décompressé là :

| Environnement | Source |
| --- | --- |
| Python | [python-build-standalone](https://github.com/astral-sh/python-build-standalone) (versions complètes et stables) |
| JDK | [Eclipse Temurin](https://adoptium.net) (dernière version de la version majeure choisie) |
| Node.js | [nodejs.org](https://nodejs.org/dist) (dernière version de chaque version majeure à partir de 18) |

- Pour chaque environnement, la fenêtre propose **celui du système** (le PATH) ou l'une des versions installées dans le projet. Le choix est gardé dans `.atelier/config.json` (`runtimes`), qui peut être versionné ; les environnements eux-mêmes sont listés dans `.atelier/.gitignore`.
- **Les nouveaux terminaux du projet** ont les environnements choisis en tête de leur `PATH`, plus `JAVA_HOME` pour un JDK. Un `.venv` (ou `venv`) à la racine est activé aussi (`VIRTUAL_ENV`). Les terminaux WSL ne sont pas touchés.
- Les versions fixées par les fichiers habituels sont lues : `.python-version`, `.java-version`, `.sdkmanrc` (`java=`), `.nvmrc`, `.node-version`. Quand aucune version installée ne donne celle demandée, une notification le dit une fois et la fenêtre propose de l'installer (`3.12` installe la dernière 3.12, `lts/*` la dernière LTS).
- **Créer .venv** lance `python -m venv .venv` avec le Python du projet.
- La fenêtre npm affiche la version de Node.js du projet quand il y en a une.

Rechercher partout : **Runtimes : afficher les environnements du projet**, **Python : créer un environnement virtuel (.venv)**.
