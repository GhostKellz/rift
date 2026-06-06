#!/bin/sh
# Install (or upgrade) the Rift Pager plasmoid for the current user.
#
# Installs the KPackage at:
#   $XDG_DATA_HOME/plasma/plasmoids/dev.ghostkellz.riftpager
# via kpackagetool6, then nudges plasmashell to pick it up. The pager is a
# self-contained QML applet with no daemon dependency, so this touches nothing
# global and makes no destructive edits.

set -eu

repo_root=$(cd "$(dirname "$0")/.." && pwd)
pkg_src="$repo_root/rift-pager"
plasmoid_id="dev.ghostkellz.riftpager"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install_dir="$data_home/plasma/plasmoids/$plasmoid_id"

if [ ! -f "$pkg_src/metadata.json" ]; then
    echo "error: $pkg_src/metadata.json missing; run from the repo" >&2
    exit 1
fi

echo "==> Installing $plasmoid_id"
if command -v kpackagetool6 >/dev/null 2>&1; then
    # Upgrade if already present, install otherwise — both are idempotent.
    if [ -d "$install_dir" ]; then
        kpackagetool6 --type Plasma/Applet --upgrade "$pkg_src"
    else
        kpackagetool6 --type Plasma/Applet --install "$pkg_src"
    fi
else
    echo "warning: kpackagetool6 not found; copying the package by hand" >&2
    rm -rf "$install_dir"
    mkdir -p "$(dirname "$install_dir")"
    cp -r "$pkg_src" "$install_dir"
fi

echo "==> Done. Add it to a panel:"
echo "    right-click a panel -> Add Widgets -> search 'Rift Pager'."
echo "    If it doesn't appear yet, refresh plasmashell:"
echo "        kquitapp6 plasmashell && (kstart plasmashell >/dev/null 2>&1 &)"
