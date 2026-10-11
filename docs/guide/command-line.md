# Command line

Like `code` for VS Code, the `atelier` command opens folders and files in the IDE:

```bash
atelier .              # the folder becomes the project
atelier src/main.rs    # the file opens in the IDE that is already open
atelier -n ~/other     # forces a new window
```

When an IDE is already open, the command hands it the path (to the window whose project holds the file, otherwise the last one used) and returns at once; a folder goes through the “this window / new window” choice. With no IDE open, it starts one.

The command stays silent and instant: it shows no startup window, and looks for an update at most every six hours.

## Where the command comes from

| Installation | Command |
| --- | --- |
| Windows installer | `bin\atelier.cmd` (cmd, PowerShell) and `bin\atelier` (Git Bash). Only the `bin` folder of the installation goes on the PATH, as VS Code does. |
| Launcher on Linux or macOS | A link `~/.local/bin/atelier` made at the first install |
| Launcher on Windows, without the installer | The folder of the launcher, added to the user PATH at the first install |

`atelier --install-cli` puts the command on the PATH again. Open a new terminal afterwards: the PATH of a terminal is read when it starts.
