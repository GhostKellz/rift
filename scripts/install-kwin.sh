#!/bin/sh
# Dev install: build the rift-kwin script from source, install it for the current
# user, then hand the KDE integration (enable, free Meta+L, clear stale shortcut
# records) to `riftctl setup`.
#
# This is the from-source path. The packaged install drops the script under
# /usr/share and you run `riftctl setup` directly; here we build and stage it in
# the user data dir first, then call the freshly built riftctl with --no-service
# (a dev checkout has no system riftd.service — start `riftd` from target/).

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

echo "==> Installing the script to $install_dir"
rm -rf "$install_dir"
mkdir -p "$install_dir/contents/code"
cp "$kwin_src/metadata.json" "$install_dir/metadata.json"
cp "$kwin_src/contents/code/main.js" "$install_dir/contents/code/main.js"

echo "==> Building riftctl"
cargo build --release --manifest-path "$repo_root/Cargo.toml" -p riftctl

riftctl="$repo_root/target/release/riftctl"
if [ ! -x "$riftctl" ]; then
    riftctl=riftctl  # fall back to PATH
fi

echo "==> Running riftctl setup (KDE integration)"
# --no-service: a dev checkout has no /usr/lib/systemd/user/riftd.service.
# The shortcut-clear step needs a running daemon to enumerate ids, so start
# `riftd` first if you want it to take; otherwise re-run this once it is up.
"$riftctl" setup --no-service || true

echo "==> Done. Start the daemon with: $repo_root/target/release/riftd"
echo "    (or: cargo run -p riftd). If shortcuts look stale, re-run with riftd"
echo "    running: $riftctl setup --no-service"
