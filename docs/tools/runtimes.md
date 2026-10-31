# Runtimes

The built-in **Runtimes** plugin installs a Python, JDKs and Node.js versions **inside the project**. Its **Runtimes** window is on the right rail; the [Jupyter](./jupyter) and [Node.js](./node) plugins use what it installs.

## Runtimes of the project

Each runtime is downloaded into `.atelier/runtimes/<python|jdk|node>/<version>`, checked against its SHA-256 and unpacked there:

| Runtime | Source |
| --- | --- |
| Python | [python-build-standalone](https://github.com/astral-sh/python-build-standalone) (full builds, stable releases) |
| JDK | [Eclipse Temurin](https://adoptium.net) (latest build of the chosen feature release) |
| Node.js | [nodejs.org](https://nodejs.org/dist) (latest release of each major version from 18) |

- For each runtime, the window offers **the one of the system** (the PATH) or one of the versions installed in the project. The choice is kept in `.atelier/config.json` (`runtimes`), which can be versioned; the runtimes themselves are listed in `.atelier/.gitignore`.
- **New terminals of the project** get the chosen runtimes first on their `PATH`, plus `JAVA_HOME` for a JDK. A `.venv` (or `venv`) at the root is activated as well (`VIRTUAL_ENV`). WSL terminals are left alone.
- The versions pinned by the usual files are read: `.python-version`, `.java-version`, `.sdkmanrc` (`java=`), `.nvmrc`, `.node-version`. When none of the installed versions gives the pinned one, a notification says so once and the window offers to install it (`3.12` installs the newest 3.12, `lts/*` the newest LTS).
- **Create .venv** runs `python -m venv .venv` with the Python of the project.
- The npm window shows the Node.js version of the project when there is one.

Search Everywhere: **Runtimes: Show Project Runtimes**, **Python: Create a Virtual Environment (.venv)**.
