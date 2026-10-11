# JavaScript & TypeScript

The editor offers completion, tooltips and error underlining for JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`) and TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`). A local analysis with tree-sitter works as soon as a file opens; when `typescript-language-server` is available, it answers instead, with the precision of TypeScript.

## Completion

It opens while you type (after a character or a dot) or with **Ctrl+Space**, and offers:

- the variables, functions, classes, parameters and imports in scope;
- the globals of JavaScript and Node.js, the keywords, and code templates (`log`, `fori`, `func`, `try`…);
- the members of the type of the value before the dot: strings, arrays, `Math`, `console`, `Promise`, `Map`, object literals, local classes, `this`;
- the paths in `import … from '…'` and `require('…')`: files, folders, `node_modules`, the dependencies of `package.json`, Node modules.

Arrows to choose, **Enter** to insert, **Tab** to replace the rest of the word, **Esc** to close.

## Tooltips and errors

- Resting the pointer on a name shows its declaration, its type and its JSDoc comment (with the types of `@param` and `@type`).
- Syntax errors are underlined in red, and counted in the status bar.

## Navigation

- **Ctrl+click** (or **Ctrl+B**) on a name goes to its declaration, also in another file or an imported module; on a declaration, it lists its usages in a popup.
- Holding **Ctrl** over a name underlines it as a link and shows the line of its declaration.
- **Ctrl+Shift+B** goes to the type declaration, **Ctrl+Alt+B** to the implementations (language server), **Alt+F7** lists the usages.
- **Ctrl+F12** opens the file structure (functions, classes, methods), filtered as you type.
- **Ctrl+Alt+Left / Right** goes back and forward, **Ctrl+Shift+M** jumps to the matching bracket.
- The occurrences of the name under the caret are highlighted, as is the pair of brackets.

## Quick actions and rename

- **Alt+Enter** on an undeclared name offers to import it from the other files of the project (`import { x } from './f.js'` or `require`, joining an existing import of the same module), from a Node module or from a dependency of `package.json`. With the language server, these are its quick actions.
- **Shift+F6** renames the name under the caret everywhere, including in the files of the project that import it (`import { x }` and its uses, leaving aliases and namesakes alone); `{ a }` becomes `{ a: b }`, and an invalid or taken name is refused. Other files are changed too: open ones are to be saved, closed ones are written.

## TypeScript

Same engine, with the TypeScript grammar. The types written in the code are understood: annotations, interfaces (and `extends`), aliases, enums, simple generics, `Promise<T>` and `await`, typed arrays (`users.map(u => u.…)`, `for…of`), `as`, private class members and constructor properties.

Completion offers types in a type position, the expected properties of a typed object literal, and the TypeScript keywords and templates; tooltips, navigation, the file structure and Alt+Enter (imports without extension, `@types`) take types into account, and TypeScript keywords are colored.

## Language server

When `typescript-language-server` is found (in the project's `node_modules/.bin`, in `~/.atelier/lsp` or on the PATH), it takes over, for `.ts` and `.tsx` files too; the local analysis stays the fallback. **JavaScript: Install Language Server** in Search Everywhere installs it in `~/.atelier/lsp` (Node.js required), without touching the global installation. The status bar shows the active engine (`tsserver` or `JS: local analysis`). WSL projects use the local analysis.
