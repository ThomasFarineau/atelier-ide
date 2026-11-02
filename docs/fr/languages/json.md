# Schémas JSON et YAML

Atelier lit lui-même les fichiers `.json` : erreurs de syntaxe pendant la frappe, et **Reformater le code** (Ctrl+Alt+L) avec l'indentation du fichier et l'ordre des clés. Les commentaires et les virgules finales, qu'ont les réglages de nombreux outils (`tsconfig.json`, `.vscode/settings.json`…), ne sont pas des erreurs.

## Schémas {#schemas}

Un fichier JSON ou YAML peut avoir un schéma (JSON Schema). Il a alors :

- ses valeurs vérifiées : types, valeurs permises, motifs, clés obligatoires et inconnues, en avertissements dans l'éditeur et dans **Problèmes** ;
- la complétion de ses clés, avec leur description, et de leurs valeurs (les valeurs permises, `true` et `false`), avec **Ctrl+Espace** ou pendant la frappe ;
- la description d'une clé sous la souris.

D'où vient le schéma d'un fichier :

1. de celui qu'il nomme : sa clé `$schema` en JSON, un commentaire `# yaml-language-server: $schema=…` en YAML. C'est un fichier du projet, en chemin relatif au fichier (`"$schema": "./schemas/config.json"`), ou l'adresse d'un schéma qu'apporte un plugin (`https://json.schemastore.org/package.json`). Les autres adresses ne sont pas téléchargées ;
2. sinon, du nom ou de la place du fichier, pour les schémas qu'apportent les plugins :

| Fichier | Plugin |
| --- | --- |
| `package.json` | [Node.js](../plugins/node) |
| `.github/workflows/*.yml`, `.github/dependabot.yml` | [GitHub](../plugins/git#github) |

Un fichier YAML sans schéma n'est pas vérifié : ce peut être un modèle (Helm…) plutôt que du YAML.

Un schéma disparaît avec le plugin qui l'apporte, quand celui-ci est désactivé dans la fenêtre **Plugins**.

Ce qui est lu d'un schéma : `type`, `enum`, `const`, `properties`, `patternProperties`, `additionalProperties`, `required`, `items`, `prefixItems`, `pattern`, `minLength`, `maxLength`, `minItems`, `maxItems`, `minimum`, `maximum`, `allOf`, `anyOf`, `oneOf`, `description` et les `$ref` internes au schéma.
