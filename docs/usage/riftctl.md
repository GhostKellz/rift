# riftctl

`riftctl` is the command-line client. It connects to the daemon over the Unix
socket and speaks the same protocol as the KWin script.

## Inspection

| Command          | Effect                                                         |
| ---------------- | ------------------------------------------------------------- |
| `riftctl status` | Print version, protocol, uptime, and live cell/window counts. |
| `riftctl config` | Print the effective config, source path, and whether loaded.  |
| `riftctl keys`   | Print the effective keybinding table (defaults plus `[keys]`).|

## Control

| Command                      | Effect                                             |
| ---------------------------- | -------------------------------------------------- |
| `riftctl focus <dir>`        | Move focus left/right/up/down.                     |
| `riftctl move <dir>`         | Swap the focused window with its neighbor, or send it to the adjacent output at a cell edge. |
| `riftctl resize <dir>`       | Adjust the master split (`left`/`right`); `up`/`down` reserved. |
| `riftctl layout <kind>`      | Switch the focused cell's layout.                  |
| `riftctl master-ratio <d>`   | Adjust master ratio by delta (clamped 0.05–0.95).  |
| `riftctl master-count <d>`   | Adjust master window count by delta (floored at 1).|

`<dir>` is one of `left`, `right`, `up`, `down`. `<kind>` is one of `tile`,
`monocle`, `columns`, `spiral`, `threecolumn`, `floating`.

## State

| Command          | Effect                                                       |
| ---------------- | ------------------------------------------------------------ |
| `riftctl reset`  | Force full re-materialization from the last topology.        |
| `riftctl reload` | Re-read `riftrc`; a rejected reload prints the diagnostic.   |

A rejected reload exits non-zero, so it composes cleanly in scripts and editor
save hooks.

## Setup

| Command         | Effect                                                        |
| --------------- | ------------------------------------------------------------- |
| `riftctl setup` | Per-user KDE integration; idempotent (see [install](../install.md)). |

`setup` enables the KWin script, frees `Meta+L` (relocating KDE Lock Session to
`Ctrl+Alt+L`), clears stale rift shortcut records, and enables+starts the
`systemd --user` unit. Pass `--no-service` to skip the unit (e.g. a dev checkout
that runs `riftd` from `target/`).
