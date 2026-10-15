---
default: patch
---

# Renommer un nom importé renomme aussi sa déclaration

En JavaScript et TypeScript, Renommer (Maj+F6) sur une fonction importée, depuis un fichier qui l'utilise, ne changeait que ce fichier : l'import ne correspondait plus à rien. Le fichier qui la déclare et les autres fichiers qui l'importent sont maintenant renommés aussi, comme quand on renomme depuis la déclaration.
