# 3D Model Viewer (`gltf-viewer`)

A native plugin of Atelier: glTF and GLB files (`.gltf`, `.glb`) open in a 3D viewer instead of
the text editor, with an orbit camera and a player for their animations. It was a built-in
plugin of the IDE; it is now a dynamic library the IDE loads when the plugin is turned on in a
project (see `doc/en-US.md` for what it does).

## Build and install

The library must be built like the IDE that loads it (same compiler, profile and crates):
build both from the root of the repository, then copy the manifest and the library into the
project.

```powershell
# Windows: builds the IDE and the plugin, and installs the plugin into a project
./plugins/gltf-viewer/install.ps1 -Project C:\path\to\project            # release
./plugins/gltf-viewer/install.ps1 -Project C:\path\to\project -Dev       # for `cargo run`
```

```sh
# Linux, macOS
./plugins/gltf-viewer/install.sh /path/to/project            # release
./plugins/gltf-viewer/install.sh /path/to/project --debug    # for `cargo run`
```

The project then has `.atelier/plugins/gltf-viewer/plugin.json` and the library beside it. In
Atelier, open **Plugins**, click **Reload** and enable **3D Model Viewer**.

## Layout

- `src/lib.rs`: the plugin (`declare_plugin!`), the camera, the animation player and the view.
- `src/scene.rs`: reading the model, its node tree, skins and animations, on the CPU.
- `src/render.rs`: drawing it with OpenGL, in a paint callback of egui_glow.
- `locales/`: its texts, in the language of the IDE.
- `plugin.json`: the manifest copied into the project.
