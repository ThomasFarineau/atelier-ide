---
default: minor
---

# Éditeur, navigation et intelligence du code d'IntelliJ

**Éditeur.** Plusieurs curseurs : Alt+J ajoute l'occurrence suivante de la sélection, Alt+Maj+J retire la dernière, Ctrl+Alt+Maj+J les prend toutes, Alt+Maj+clic ajoute ou retire un curseur. La sélection en colonne se fait en glissant avec Alt, ou après Alt+Maj+Inser. Ctrl+Maj+Haut/Bas déplace l'instruction entière, bloc compris. Ctrl+Alt+T entoure la sélection (accolades, `if`, `try`, guillemets…) et propose vos modèles. Le fichier `templates.json` du dossier de données ajoute des modèles de code à la complétion et à « Entourer de ». L'éditeur peut être partagé en deux (Partager à droite ou en bas, dans le menu des onglets), les lignes qui ouvrent les blocs restent en haut, le fil d'Ariane montre où est le curseur. F11 pose un signet, Maj+F11 les liste, Ctrl+Maj+1…9 en pose un numéroté et Ctrl+1…9 y va. L'historique local garde une copie à chaque enregistrement (« Afficher l'historique local ») et montre en choisissant une version ce que sa restauration changerait, avant de la restaurer. Les fichiers de 4 à 64 Mo s'ouvrent en lecture seule.

**Navigation.** Ctrl+Maj+A cherche une action, Ctrl+Maj+E liste les emplacements récents avec leur ligne, Ctrl+Tab change de fichier, Ctrl+Q affiche la documentation, Ctrl+Maj+I la définition, Ctrl+H et Ctrl+Alt+H les hiérarchies de types et d'appels. Ctrl+P ne sert plus à Rechercher partout : il affiche les paramètres de l'appel.

**Intelligence du code.** Les indications du serveur de langage (types, noms des arguments) s'affichent dans le code. Ctrl+P donne la signature de l'appel, avec le paramètre courant. Ctrl+Alt+Maj+T liste les refactorings du serveur pour la sélection. « Inspecter le code » cherche les erreurs de syntaxe de tous les fichiers du projet et les met dans Problèmes ; la fenêtre Commit prévient quand le code à valider en contient. L'orthographe des commentaires, des chaînes et des fichiers Markdown est vérifiée (anglais et français) : Alt+Entrée propose les mots proches et l'ajout au dictionnaire. Ces réglages sont dans Éditeur › Général.

**Correction.** Rechercher partout (double Maj) ne fige plus l'IDE sur un gros projet : la liste des fichiers se fait en arrière-plan et les recherches sont gardées d'une image à l'autre.

**Modèles.** Les modèles postfixes (`valeur.if`, `.var`, `.for`, `.return`, `.log`… selon le langage) prennent l'expression avant le point. Les modèles de code, postfixes et d'entourage de l'utilisateur se modifient dans Éditeur › Modèles de code. « Entourer de » connaît aussi les blocs de Python. Alt+Entrée, Ctrl+Alt+V/C/N/M : intentions et refactorings de l'IDE (extraire une variable, une constante, une méthode, inliner). « Inspecter le code » ajoute les erreurs des serveurs et de `tsc`.
