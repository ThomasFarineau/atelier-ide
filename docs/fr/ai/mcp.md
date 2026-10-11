# Serveur MCP et sessions

Le plugin **Atelier Super Intelligence Tools** fait d'Atelier un serveur MCP (Model Context Protocol), pour qu'un agent IA lancé dans un de ses terminaux puisse se servir de l'IDE : HTTP sur `127.0.0.1` seulement, protégé par un secret conservé d'une session à l'autre.

## Connecter un agent

Les terminaux de l'IDE définissent `ATELIER_MCP_URL` et `ATELIER_MCP_TOKEN`. Quand vous y lancez `claude`, `gemini` ou `codex`, l'IDE retient la touche Entrée et propose de connecter l'agent. **Connecter** écrit le serveur :

| Agent | Fichier |
| --- | --- |
| Claude Code | `.mcp.json`, et l'approbation dans `.claude/settings.local.json` |
| Gemini CLI | `.gemini/settings.json` |
| Codex | `.codex/config.toml` |

Aucun fichier ne contient le secret (`${ATELIER_MCP_TOKEN}` ou `bearer_token_env_var`). Claude et Gemini lisent aussi l'adresse dans `${ATELIER_MCP_URL}`, donc leurs fichiers restent valables quand le port change. Codex ne développe pas les variables : son fichier contient l'adresse du moment et se réécrit quand elle change (le port est conservé d'une session à l'autre, ce qui l'évite le plus souvent), mieux vaut donc ne pas le versionner. La réponse est retenue par projet, et les commandes **MCP : connecter…** de Rechercher partout font la même chose à la demande.

Un autre client MCP peut s'y connecter avec l'adresse et le secret (`mcp.json` du dossier de données d'Atelier) : `Authorization: Bearer <secret>`.

## Outils

| Outil | Donne |
| --- | --- |
| `get_project_info` | Le projet |
| `get_open_files` | Les fichiers ouverts |
| `get_active_editor` | L'éditeur actif, le curseur et la sélection |
| `open_file` | Ouvre un fichier, à une ligne |
| `get_problems` | Les erreurs et avertissements des fichiers ouverts |
| `search_in_project` | Cherche dans le projet, hors du fil de l'interface (la fenêtre ne se fige pas) |
| `i18n` | Les [traductions](../tools/translations) |
| `graph_overview`, `graph_query`, `graph_node`, `graph_path` | Le [graphe de contexte](./context-graph) |
| `git_commit_plan`, `git_diff`, `git_commit`, `git_rework_plan`, `git_show`, `git_rework_apply` | Les [commits par l'IA](./commits) |
| `session_note`, `get_session_context`, `list_sessions` | Les sessions d'IA, ci-dessous |

## Sessions d'IA

Chaque connexion d'un agent au serveur est une **session du projet**, enregistrée dans `.atelier/sessions/` (ignoré par git grâce à `.atelier/.gitignore`). La fenêtre **Sessions IA** (**Alt+7**) les liste, avec l'agent, l'heure, les appels faits à l'IDE et les notes laissées.

Le protocole ne montre pas la conversation, seulement les appels d'outils. C'est donc l'IDE qui tient le contexte : l'agent est invité (dans les instructions du serveur) à noter l'objectif, les décisions, les tâches restantes et l'avancement avec `session_note`.

- **Reprendre avec…** écrit ce contexte dans `.atelier/sessions/<id>.md`, connecte l'agent choisi (Claude Code, Codex ou Gemini CLI, pas forcément celui qui avait commencé) et le lance dans un nouveau terminal, avec une première demande qui renvoie à ce fichier. L'agent peut aussi appeler `get_session_context` (l'`id` d'une session) et `list_sessions`.
- **Copier le contexte** le met dans le presse-papiers, pour n'importe quel autre outil.

## Agents installés

L'onglet **IA installées** de la fenêtre montre l'état de Claude Code, Codex et Gemini CLI :

- installé ou non (le programme trouvé dans le PATH, sa version, sinon un bouton qui l'installe avec npm) ;
- connecté ou non au serveur MCP de l'IDE pour ce projet (avec un bouton pour le connecter) ;
- son usage, lu dans les fichiers qu'il garde sur l'ordinateur (`~/.claude/projects`, `~/.codex/sessions`, `~/.gemini/tmp`) : tokens sur 24 heures, 7 et 30 jours (entrée / cache / sortie au survol), la part de ce projet, les sessions et réponses, les modèles utilisés, la dernière utilisation, les sessions et appels passés par Atelier et, pour Codex, les limites d'usage de l'abonnement (fenêtre de 5 heures et semaine, avec leur réinitialisation).

Rien n'est demandé aux serveurs des agents : seul l'usage de cet ordinateur est compté. Les fichiers ne sont relus que s'ils ont changé, en arrière-plan, au plus une fois par minute tant que l'onglet est affiché.
