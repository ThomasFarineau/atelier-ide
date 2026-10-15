# Runtimes & Jupyter

Le plugin intégré **Runtimes & Jupyter** installe un Python, des JDK et des versions de Node.js **dans le projet lui-même**, et exécute les notebooks Jupyter avec le Python du projet. Sa fenêtre **Runtimes** est sur le rail de droite.

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

## Notebooks Jupyter

Les fichiers `.ipynb` s'ouvrent en notebook (le sélecteur de disposition en haut à droite affiche le JSON à côté ou à la place) :

- Les cellules de code se modifient sur place, avec coloration ; **▶** ou **Maj+Entrée** exécute une cellule, la barre d'outils les exécute toutes, interrompt ou redémarre le noyau et efface les sorties.
- Les sorties sont écrites dans le notebook comme le fait Jupyter : texte, erreurs (sans leurs couleurs), images PNG, JPEG et SVG, Markdown. Enregistrez le fichier pour les garder.
- Les cellules Markdown sont rendues ; double-cliquez sur l'une d'elles pour la modifier. Les cellules peuvent être ajoutées, déplacées, supprimées et changées entre code et Markdown.
- Le noyau tourne avec le Python du projet : son `.venv`, sinon son Python installé, sinon celui du système. Il lui faut `ipykernel` et `jupyter_client` : quand ils manquent, **Installer Jupyter** lance `pip install` dans un terminal.
- **Ouvrir dans JupyterLab** démarre `python -m jupyter lab` sur le notebook (JupyterLab doit être installé).

Rechercher partout : **Runtimes : afficher les environnements du projet**, **Jupyter : nouveau notebook**, **Jupyter : installer Jupyter dans le Python du projet**, **Jupyter : ouvrir JupyterLab**, **Python : créer un environnement virtuel (.venv)**.
