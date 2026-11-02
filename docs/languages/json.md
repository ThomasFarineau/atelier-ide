# JSON and YAML schemas

Atelier reads `.json` files itself: syntax errors as you type, and **Reformat Code** (Ctrl+Alt+L) with the indentation of the file and the order of the keys. Comments and trailing commas, which the settings of many tools have (`tsconfig.json`, `.vscode/settings.json`…), are not errors.

## Schemas

A JSON or YAML file can have a schema (JSON Schema). It then gets:

- its values checked: types, allowed values, patterns, required and unknown keys, as warnings in the editor and in **Problems**;
- the completion of its keys, with their description, and of their values (the allowed values, `true` and `false`), with **Ctrl+Space** or as you type;
- the description of a key under the mouse.

Where the schema of a file comes from:

1. the schema it names: its `$schema` key in JSON, a `# yaml-language-server: $schema=…` comment in YAML. It is a file of the project, as a path relative to the file (`"$schema": "./schemas/config.json"`), or the address of a schema a plugin brings (`https://json.schemastore.org/package.json`). Other addresses are not downloaded;
2. otherwise, the name or the place of the file, for the schemas the plugins bring:

| File | Plugin |
| --- | --- |
| `package.json` | [Node.js](../plugins/node) |
| `.github/workflows/*.yml`, `.github/dependabot.yml` | [GitHub](../plugins/git#github) |

A YAML file without a schema is not checked: it may be a template (Helm…) rather than YAML.

A schema goes away with the plugin that brings it, when it is turned off in the **Plugins** window.

What is read of a schema: `type`, `enum`, `const`, `properties`, `patternProperties`, `additionalProperties`, `required`, `items`, `prefixItems`, `pattern`, `minLength`, `maxLength`, `minItems`, `maxItems`, `minimum`, `maximum`, `allOf`, `anyOf`, `oneOf`, `description` and the `$ref`s inside the schema.
