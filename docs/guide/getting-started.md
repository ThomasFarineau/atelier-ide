# Installation

<DownloadButton />

## Windows

Download **`atelier-setup-windows-x86_64.exe`** from the [latest release](https://github.com/ThomasFarineau/atelier-ide/releases/latest) and run it. It installs Atelier for your user only, without administrator rights, in `%LOCALAPPDATA%\Programs\Atelier`, with:

- a Start menu shortcut (and a desktop one if you tick it);
- the `atelier` command on the PATH (see [Command line](./command-line));
- an uninstaller in the Windows settings.

The IDE is already in the installer: nothing is downloaded at the first start.

::: tip SmartScreen
The installer is not signed yet. Windows may show “Windows protected your PC”: click **More info › Run anyway**.
:::

## Linux and macOS

Download the launcher of your platform from the [latest release](https://github.com/ThomasFarineau/atelier-ide/releases/latest) once (`atelier-linux-x86_64`, `atelier-macos-aarch64` or `atelier-macos-x86_64`), put it in a folder of its own and start it. It downloads the IDE next to itself, then adds the `atelier` command to `~/.local/bin`.

On macOS, a file downloaded by the browser is quarantined:

```bash
xattr -d com.apple.quarantine atelier-macos-aarch64
chmod +x atelier-macos-aarch64
```

## Updates

You always start the **launcher** (`atelier`), never the IDE itself: the shortcuts and the command do it for you. Like Discord or VS Code, it shows a small window with the logo, the current step and a progress bar, and closes once the IDE is started. At each start it:

1. asks GitHub for the latest release (4 seconds at most; offline, it goes on);
2. if it is newer than `app/VERSION` (or the IDE is not there yet), downloads the archive of the platform and checks it against `SHA256SUMS`;
3. swaps the `app/` folder in one go, then starts the IDE. The IDE is not running at that moment, so nothing is ever locked on Windows. A wrong archive changes nothing: the old version starts.

The launcher updates itself with each release. If something goes wrong, the window stays open with the reason and the **Retry** and **Close** buttons. What the last start did is in `bootstrap.log`, next to the launcher.

| Variable | Effect |
| --- | --- |
| `ATELIER_NO_UPDATE=1` | Skips the check for an update |
| `ATELIER_REPO=owner/repo` | Takes the releases of another repository |
| `ATELIER_GITHUB_TOKEN` | Gives access to a private repository |

## Build from source

You need stable Rust and a native toolchain. On Windows, install the Visual Studio Build Tools with the C++ workload.

```bash
cargo run                         # start the IDE
cargo run -- path/to/project      # open a project right away
cargo run -- examples/demo        # the multi-language demo project
cargo build --release             # target/release/atelier-ide(.exe)
```

## Where Atelier keeps its data

Language, theme, font size, recent projects and enabled plugins are saved in `settings.json`, in the data folder of Atelier:

| OS | Folder |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Atelier` |
| macOS | `~/Library/Application Support/Atelier` |
| Linux | `$XDG_DATA_HOME/Atelier` (`~/.local/share/Atelier` by default) |

Settings of a project (run configurations, translations source, AI sessions, context graph) live in its `.atelier/` folder.
