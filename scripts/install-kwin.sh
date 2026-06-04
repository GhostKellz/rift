#!/bin/sh
# Build, install, and enable the rift-kwin script for the current user.
#
# Assembles a KWin KPackage at:
#   $XDG_DATA_HOME/kwin/scripts/rift/   (default ~/.local/share/...)
# then enables it via kwriteconfig6 and asks KWin to reconfigure.

set -eu

repo_root=$(cd "$(dirname "$0")/.." && pwd)
kwin_src="$repo_root/rift-kwin"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install_dir="$data_home/kwin/scripts/rift"

echo "==> Building rift-kwin"
npm --prefix "$kwin_src" install
npm --prefix "$kwin_src" run build

if [ ! -f "$kwin_src/contents/code/main.js" ]; then
    echo "error: build did not produce contents/code/main.js" >&2
    exit 1
fi

echo "==> Installing to $install_dir"
rm -rf "$install_dir"
mkdir -p "$install_dir/contents/code"
cp "$kwin_src/metadata.json" "$install_dir/metadata.json"
cp "$kwin_src/contents/code/main.js" "$install_dir/contents/code/main.js"

echo "==> Enabling KWin script"
if command -v kwriteconfig6 >/dev/null 2>&1; then
    kwriteconfig6 --file kwinrc --group Plugins --key rift-kwinEnabled true
else
    echo "warning: kwriteconfig6 not found; enable 'rift-kwin' manually" >&2
fi

if command -v qdbus6 >/dev/null 2>&1; then
    qdbus6 org.kde.KWin /KWin reconfigure || true
else
    echo "warning: qdbus6 not found; reconfigure KWin manually" >&2
fi

echo "==> Done. Start the daemon with: riftd"
