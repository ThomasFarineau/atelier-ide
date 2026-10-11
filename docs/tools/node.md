# Node.js (npm, pnpm, Yarn, Bun)

The built-in Node.js plugin adds the **npm** window on the right rail, only in a project that has a `package.json`. It lists every `package.json` of the project (root, workspaces and subfolders, without `node_modules`), with their scripts in the order of the file.

The package manager of each `package.json` is the one of its `packageManager` field, otherwise the one of the lock file in its folder or a parent folder (`bun.lock(b)`, `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json`), otherwise npm.

- **Double-click** (or ▶) on a script: it runs in a new terminal, from the folder of its `package.json` (`pnpm run build`…). On Windows, the `.cmd` launchers are used so that the PowerShell execution policy blocks nothing.
- Context menu of a `package.json`: install or update the dependencies, see the outdated packages, open the file. Of a script: run it or go to its line.
- Dependencies declared but not installed are reported once, by a notification.
- Search Everywhere: **Node.js: Install Dependencies**, **Node.js: npm Scripts**.

What the window starts becomes a temporary [run configuration](../guide/running), to run again with Shift+F10.
