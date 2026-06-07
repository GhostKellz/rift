# Installing Rift

Rift targets KDE Plasma 6 on Wayland (KWin). There are two paths: the Arch
package (recommended for daily use) and a from-source dev install.

## Arch Linux (AUR)

Install the `rift` package with your AUR helper, or build it by hand:

```sh
# With an AUR helper:
paru -S rift        # or: yay -S rift

# Or manually from the PKGBUILD in this repo:
cd packaging
makepkg -si
```

The package installs:

| Path                                             | What                         |
| ------------------------------------------------ | ---------------------------- |
| `/usr/bin/riftd`, `/usr/bin/riftctl`             | Daemon and CLI               |
| `/usr/share/kwin/scripts/rift`                   | The KWin script              |
| `/usr/share/plasma/plasmoids/dev.ghostkellz.riftpager` | The pager applet       |
| `/usr/lib/systemd/user/riftd.service`            | The `systemd --user` unit    |

### Finish setup

The package cannot touch per-user KDE state at install time, so run this once in
your Plasma session:

```sh
riftctl setup
```

This enables the KWin script, relocates KDE's Lock Session off `Meta+L` to
`Ctrl+Alt+L` (so rift can use `Meta+L` for focus-right), clears stale rift
shortcut records, and enables+starts `riftd.service`. It is idempotent — re-run it
any time shortcuts look wrong.

### Add the pager

Right-click a panel → **Add Widgets** → search **Rift Pager**. If it does not
appear yet, refresh plasmashell:

```sh
kquitapp6 plasmashell && (kstart plasmashell >/dev/null 2>&1 &)
```

## From source (dev)

Requires a Rust toolchain (edition 2024, rustc ≥ 1.96) and Node/npm for the KWin
script bundle.

```sh
# Build, install the KWin script for your user, and run riftctl setup --no-service.
./scripts/install-kwin.sh

# Install the pager applet.
./scripts/install-pager.sh

# Start the daemon (a dev checkout has no system unit):
cargo run -p riftd        # or: ./target/release/riftd
```

The dev script installs the script under `~/.local/share/kwin/scripts/rift` and
delegates the KDE integration to `riftctl setup --no-service`. The shortcut-clear
step needs a running daemon to enumerate ids, so if you start `riftd` after the
script, re-run `riftctl setup --no-service` once it is up.

See [building.md](building.md) for the full build and test workflow, and
[troubleshooting/reliability.md](troubleshooting/reliability.md) for how rift
behaves across restarts, suspend, and monitor hotplug.
