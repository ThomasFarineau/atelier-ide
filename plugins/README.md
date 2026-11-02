# Native plugins

Each native plugin of Atelier has a repository of its own (`atelier-ide-org/plugin-<name>`). Clone one here to build it with the IDE: this folder is ignored by git (but this file), and every folder in it joins the Cargo workspace.

```sh
git clone git@github.com:atelier-ide-org/plugin-gltf-viewer.git plugins/gltf-viewer
cargo build --workspace
```

The IDE is built and released without its plugins. The CI of each plugin builds it for each release of the IDE, from the IDE at the tag of that release, and publishes it in `atelier-ide-org/releases` under the tag `plugins/<name>/vX.Y.Z` (the version of the IDE).
