# Jupyter notebooks

The built-in **Jupyter** plugin runs Jupyter notebooks with the Python of the project, the one the [Runtimes](./runtimes) plugin chose.

`.ipynb` files open as a notebook (the layout switch at the top right shows the JSON next to it or instead of it):

- Code cells are edited in place with highlighting; **▶** or **Shift+Enter** runs a cell, the toolbar runs all of them, interrupts or restarts the kernel and clears the outputs.
- Outputs are written into the notebook as Jupyter does: text, errors (without their colors), PNG, JPEG and SVG images, Markdown. Save the file to keep them.
- Markdown cells are rendered; double-click one to edit it. Cells can be added, moved, deleted and changed between code and Markdown.
- The kernel runs with the Python of the project: its `.venv`, else its Python runtime, else the one of the system. It needs `ipykernel` and `jupyter_client`: when they are missing, **Install Jupyter** runs `pip install` in a terminal.
- **Open in JupyterLab** starts `python -m jupyter lab` on the notebook (JupyterLab must be installed).

Search Everywhere: **Jupyter: New Notebook**, **Jupyter: Install Jupyter in the Project Python**, **Jupyter: Open JupyterLab**.
