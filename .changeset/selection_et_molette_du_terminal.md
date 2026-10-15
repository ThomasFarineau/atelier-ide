---
default: patch
---

# Sélection à la souris, copie et molette dans le terminal

Une sélection à la souris, dans l'éditeur comme dans le terminal, commençait quelques pixels après l'endroit cliqué : le premier caractère manquait, et Ctrl+C copiait parfois une sélection vide dans le terminal. Elle commence maintenant là où le bouton a été enfoncé.

Dans le terminal, la molette fonctionne aussi dans les programmes qui prennent tout l'écran (éditeurs, pagers, IA) : elle leur est envoyée comme des événements de molette s'ils suivent la souris, sinon comme les flèches ↑ et ↓.
