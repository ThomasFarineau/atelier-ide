# Translations (i18n)

The built-in **Translations** plugin manages the translation files of a project from a **source**: the reference language, saved in `.atelier/i18n.json`.

## The source

The source can be:

- **a file**; the other languages are found next to it:
  - `locales/fr.json`, `locales/en_US.json`: one file per language in a folder;
  - `locales/fr/common.json`, `locales/en/common.json`: one folder per language;
  - `messages.properties`, `messages_fr.properties`: Java bundles (the file without suffix is the default language);
- **the folder of a language** (`locales/fr`): each file in it, subfolders included, is a namespace, and its keys are written `namespace:key` (`common:menu.open`, `auth/login:title`), as with i18next. The other languages are the sibling folders; a namespace a language does not have yet is created with the first value written;
- **a regular expression** on the paths of the project's files (relative, with `/`), with a `(?<lang>…)` group for the language and, optionally, `(?<ns>…)` for the namespace: `src/i18n/messages\.(?<lang>\w+)\.json`, `app/(?<ns>\w+)/lang/(?<lang>[a-z]{2})\.json`. You also give the reference language (English by default when it is there, otherwise the first one). Without `ns`, a language with several files tells them apart by their path.

JSON files can be flat (`"menu.open"`) or nested (`{"menu": {"open": …}}`).

## The Translations window

At the bottom. It first offers as source the files and folders that look like translations, the active file, or a regular expression. Then it shows every key in every language:

- missing in red;
- empty, or with other variables than the source (`{name}`, `{{count}}`, `%s`, `${x}`), in orange;
- identical to the source in grey;
- with the translated percentage of each language.

A click on a cell edits it and writes it to its file (in place, with the indentation of the file). You can add a language (a file created with every key of the source) or a key (in every language); the menu of a key opens it in the source or deletes it everywhere. The table follows the files changed outside the IDE.

## For AI agents

The **`i18n`** MCP tool (see [MCP server](../ai/mcp)) gives the same to AI agents. `action` is:

| Action | Result |
| --- | --- |
| `languages` | The languages |
| `check` | Every language compared to the source |
| `check_language` | One language in detail (with `language`) |
| `add_language` | Creates a language, with empty values or, with `fill: "source"`, the text of the source |

Results list the missing, extra, empty, different-variables and identical-to-source keys. `source` picks a source file or folder, `pattern` (with `source_language`) a regular expression.
