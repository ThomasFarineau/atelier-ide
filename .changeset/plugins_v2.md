---
default: major
---

# Plugins natifs v2 : une interface stable, et un onglet Disponibles

Les plugins natifs passent à la version 2 de leur interface (`atelier-plugin-api` 0.3) : l'IDE et un plugin ne partagent plus que des types et des fonctions C. Un plugin dessine avec son propre egui, que l'IDE nourrit et dont il peint les maillages et les textures, avec son apparence, ses polices et ses icônes. Il se compile donc seul, avec n'importe quel compilateur, et se charge dans toutes les versions d'Atelier qui lisent la même version de l'interface : plus besoin de le recompiler à chaque version de l'IDE. Les plugins de la version 1 (Atelier 0.2) ne se chargent plus : installez leur nouvelle version.

**Plugins › Disponibles** liste les plugins publiés que cet Atelier sait charger, avec **Installer**, et **Mettre à jour** quand une version plus récente est sortie.

L'explorateur de bases de données (MongoDB, PostgreSQL, MySQL) et la visionneuse Microsoft Office (Word, Excel, PowerPoint, export PDF) ne sont plus intégrés : ce sont désormais les plugins natifs `database` et `msoffice-viewer`, à installer depuis **Plugins › Disponibles**, comme la visionneuse 3D `gltf-viewer`. Les connexions enregistrées sont gardées.

Chaque notification a un bouton pour copier son message, dans le panneau des notifications et dans ses bulles.
