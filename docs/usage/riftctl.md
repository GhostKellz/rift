# riftctl

`riftctl` is the command-line client. It connects to the daemon over the Unix
socket and speaks the same protocol as the KWin script.

## Inspection

| Command          | Effect                                                         |
| ---------------- | ------------------------------------------------------------- |
| `riftctl status` | Print version, protocol, uptime, and live cell/window counts. |
| `riftctl config` | Print the effective config, source path, and whether loaded.  |

## Control

| Command                      | Effect                                             |
| ---------------------------- | -------------------------------------------------- |
| `riftctl focus <dir>`        | Move focus left/right/up/down.                     |
| `riftctl move <dir>`         | Swap the focused window with its neighbor.         |
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
