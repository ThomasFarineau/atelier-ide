# Native plugins

Each native plugin of Atelier has a repository of its own (`atelier-ide-org/plugin-<name>`), here as a submodule:

```sh
git submodule update --init plugins/database
cd plugins/database && cargo build
```

A plugin is built alone, against the plugin interface (`atelier-plugin-api`, the submodule `api/` of the IDE), never with the IDE: the IDE is built and released without its plugins, and a plugin loads in every version of the IDE that keeps the version of the interface. Its `package` script makes the file the IDE installs (Plugins, the + button): `target/<profile>/<name>.atelier-plugin`.

The CI of each plugin publishes its `<name>.atelier-plugin` and its `plugin.json` in `atelier-ide-org/releases` under the tag `plugins/<name>/vX.Y.Z` (the version of the plugin), where the IDE finds it (Plugins, Available).
