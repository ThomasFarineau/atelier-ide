---
default: minor
---

# Plugins activés par projet, dans `.atelier`

Les plugins activés et désactivés sont propres à chaque projet : leur état est écrit dans `.atelier/config.json` (section `plugins`), qui peut se partager avec le projet, et non plus dans les réglages de l'IDE. À l'ouverture d'un projet, chaque plugin qu'il ne mentionne pas encore y est inscrit : Git en `"auto"`, activé tant qu'il existe un `.git` dans le projet (un dossier, ou le fichier d'un worktree ou d'un sous-module) : il s'active de lui-même après un `git init` et se désactive si le dépôt disparaît, jusqu'à ce qu'on le règle à la main dans la fenêtre Plugins ; les autres comme ils l'étaient dans les réglages de l'IDE. Les plugins ne font plus partie des réglages exportés ou synchronisés.
