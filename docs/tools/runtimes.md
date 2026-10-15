# Runtimes & Jupyter

The built-in **Runtimes & Jupyter** plugin installs a Python, JDKs and Node.js versions **inside the project**, and runs Jupyter notebooks with the Python of the project. Its **Runtimes** window is on the right rail.

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

## Jupyter notebooks

`.ipynb` files open as a notebook (the layout switch at the top right shows the JSON next to it or instead of it):

- Code cells are edited in place with highlighting; **▶** or **Shift+Enter** runs a cell, the toolbar runs all of them, interrupts or restarts the kernel and clears the outputs.
- Outputs are written into the notebook as Jupyter does: text, errors (without their colors), PNG, JPEG and SVG images, Markdown. Save the file to keep them.
- Markdown cells are rendered; double-click one to edit it. Cells can be added, moved, deleted and changed between code and Markdown.
- The kernel runs with the Python of the project: its `.venv`, else its Python runtime, else the one of the system. It needs `ipykernel` and `jupyter_client`: when they are missing, **Install Jupyter** runs `pip install` in a terminal.
- **Open in JupyterLab** starts `python -m jupyter lab` on the notebook (JupyterLab must be installed).

Search Everywhere: **Runtimes: Show Project Runtimes**, **Jupyter: New Notebook**, **Jupyter: Install Jupyter in the Project Python**, **Jupyter: Open JupyterLab**, **Python: Create a Virtual Environment (.venv)**.
