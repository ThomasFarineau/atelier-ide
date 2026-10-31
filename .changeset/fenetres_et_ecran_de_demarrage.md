---
default: minor
---

# Fenêtres centrées, écran de démarrage et plantages expliqués

Une fenêtre s'ouvre toujours au milieu de l'écran où se trouve la souris, à sa taille par défaut ou à ce que l'écran permet. Elle ne revient plus toute petite dans le coin en haut à gauche après avoir été fermée réduite, et rouvre agrandie si la précédente l'était. Son bouton dans la barre des tâches suit la fenêtre quand elle passe sur un autre écran.

Chaque projet ouvert dans une nouvelle fenêtre est un programme à part : fermer la première fenêtre ne ferme plus les autres.

Lancé sans dossier, Atelier ouvre un écran de démarrage avec les projets récents, « Ouvrir… », « Cloner un dépôt… » et **« Nouveau projet… »** (un nom et un emplacement), au lieu d'ouvrir le dossier du programme.

Quand Atelier s'arrête de façon inattendue (erreur, mémoire saturée), la cause est écrite dans `logs/crash.log` du dossier de données et la fenêtre suivante le signale. Les lignes très longues (code minifié) ne sont affichées que sur leurs 10 000 premiers caractères : elles pouvaient prendre des gigaoctets de mémoire. Fermer l'onglet d'un gros fichier libère aussi ce que les plugins gardaient pour lui.
