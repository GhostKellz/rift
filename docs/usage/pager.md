# Rift Pager

A numbered virtual-desktop pager for the Plasma 6 panel: one box per virtual
desktop, the current one highlighted, click a box to switch. An optional badge
shows how many windows are on each desktop.

The pager is a self-contained QML plasmoid. It does **not** depend on `riftd` and
works whether or not the daemon is running — it reads desktop state from Plasma's
own task-manager library and switches through the same API the stock pager uses.

## Install

```sh
scripts/install-pager.sh
```

This installs (or upgrades) the package at
`~/.local/share/plasma/plasmoids/dev.ghostkellz.riftpager` via `kpackagetool6`.
It makes no global or destructive changes.

## Add it to a panel

1. Right-click a panel → **Add Widgets…**
2. Search for **Rift Pager** and drag it onto the panel.

If it doesn't show up in the widget list right after installing, refresh the
shell:

```sh
kquitapp6 plasmashell && (kstart plasmashell >/dev/null 2>&1 &)
```

## Usage

- **Boxes** are numbered `1..N`, one per virtual desktop, ordered as Plasma
  orders them.
- The **current desktop** is highlighted.
- **Click** a box to switch to that desktop.
- The pager follows the panel: a row on a horizontal panel, a column on a
  vertical one.

## Configuration

Right-click the pager → **Configure Rift Pager…**:

- **Window counts** — show a per-desktop window-count badge. Off by default; the
  count is best-effort (windows pinned to all desktops are counted everywhere).

## Notes

- Desktop **names** are not shown in v1 — only numbers — to keep the panel
  footprint small. Daemon-fed per-cell counts (from `riftd`) are a possible future
  enhancement once the pager opts into the IPC.
- The pager uses Plasma-version-sensitive QML APIs. It targets Plasma 6; on a
  mismatched Plasma it may fail to load. See
  [troubleshooting/diagnostics.md](../troubleshooting/diagnostics.md).
