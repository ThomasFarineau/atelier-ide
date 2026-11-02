#!/bin/sh
# Builds Atelier and this plugin together, then installs the plugin into a project:
# <project>/.atelier/plugins/gltf-viewer/{plugin.json, libgltf_viewer.so or .dylib}.
set -e
project="${1:?usage: install.sh <project> [--debug]}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
# Both at once: the IDE only loads a library built with its own features.
if [ "$2" = "--debug" ]; then
    profile=debug
    (cd "$root" && cargo build)
else
    profile=release
    (cd "$root" && cargo build --release)
fi
case "$(uname -s)" in
    Darwin) library=libgltf_viewer.dylib ;;
    *) library=libgltf_viewer.so ;;
esac
target="$project/.atelier/plugins/gltf-viewer"
mkdir -p "$target"
cp "$here/plugin.json" "$target/"
cp "$root/target/$profile/$library" "$target/"
echo "Installed into $target. In Atelier: Plugins, Reload, then enable 3D Model Viewer."
