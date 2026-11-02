# Native plugins

Each native plugin of Atelier has a repository of its own (`atelier-ide-org/plugin-<name>`). Clone one here to build it with the IDE: this folder is ignored by git (but this file), and every folder in it joins the Cargo workspace.

```sh
git clone git@github.com:atelier-ide-org/plugin-gltf-viewer.git plugins/gltf-viewer
cargo build --workspace
```

Its `package` script builds it with the IDE and makes the file the IDE installs (Plugins, the + button): `target/<profile>/<name>.atelier-plugin`.

The IDE is built and released without its plugins. The CI of each plugin builds it for each release of the IDE, from the IDE at the tag of that release, and publishes its `<name>.atelier-plugin` in `atelier-ide-org/releases` under the tag `plugins/<name>/vX.Y.Z` (the version of the IDE).

Building with a plugin here changes `Cargo.lock` (its dependencies): do not commit that change, the lockfile of the IDE is the one of the IDE alone.
