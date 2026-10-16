# Notebooks Jupyter

Le plugin intégré **Jupyter** exécute les notebooks Jupyter avec le Python du projet, celui choisi par le plugin [Runtimes](./runtimes).

Les fichiers `.ipynb` s'ouvrent en notebook (le sélecteur de disposition en haut à droite affiche le JSON à côté ou à la place) :

- Les cellules de code se modifient sur place, avec coloration ; **▶** ou **Maj+Entrée** exécute une cellule, la barre d'outils les exécute toutes, interrompt ou redémarre le noyau et efface les sorties.
- Les sorties sont écrites dans le notebook comme le fait Jupyter : texte, erreurs (sans leurs couleurs), images PNG, JPEG et SVG, Markdown. Enregistrez le fichier pour les garder.
- Les cellules Markdown sont rendues ; double-cliquez sur l'une d'elles pour la modifier. Les cellules peuvent être ajoutées, déplacées, supprimées et changées entre code et Markdown.
- Le noyau tourne avec le Python du projet : son `.venv`, sinon son Python installé, sinon celui du système. Il lui faut `ipykernel` et `jupyter_client` : quand ils manquent, **Installer Jupyter** lance `pip install` dans un terminal.
- **Ouvrir dans JupyterLab** démarre `python -m jupyter lab` sur le notebook (JupyterLab doit être installé).

Rechercher partout : **Jupyter : nouveau notebook**, **Jupyter : installer Jupyter dans le Python du projet**, **Jupyter : ouvrir JupyterLab**.
