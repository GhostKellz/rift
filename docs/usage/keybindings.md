# Keybindings

The daemon owns the keybinding table. At handshake the KWin script asks for it
(`GetKeybindings`) and registers each entry through KWin's global accelerator,
keyed by a stable id. Because KWin keys overrides by that id, rebinding a shortcut
in **System Settings → Shortcuts** survives script reloads.

Every shortcut forwards a command to the daemon; the daemon computes the result and
the script applies it. To change a default without touching System Settings, set a
`[keys]` override in `riftrc` (see [Configuration](../config/riftrc.md)); the daemon
applies it before handing the table to the script. Run `riftctl keys` to print the
effective table.

## Focus

Move keyboard focus to the neighboring window in a direction.

| Shortcut  | Action       |
| --------- | ------------ |
| `Meta+H`  | Focus left   |
| `Meta+J`  | Focus down   |
| `Meta+K`  | Focus up     |
| `Meta+L`  | Focus right  |

## Move

Swap the focused window with its neighbor in a direction.

| Shortcut        | Action      |
| --------------- | ----------- |
| `Meta+Shift+H`  | Move left   |
| `Meta+Shift+J`  | Move down   |
| `Meta+Shift+K`  | Move up     |
| `Meta+Shift+L`  | Move right  |

Moving the focused window past the edge of its cell, when a monitor lies that
way, sends it to the spatially adjacent output instead of stopping at the edge.

## Resize

Adjust the focused cell's master split. `Left`/`Right` shrink/grow the master
area; `Up`/`Down` are reserved (no-op today) but bound so the table is complete.

| Shortcut         | Action            |
| ---------------- | ----------------- |
| `Meta+Ctrl+H`    | Shrink master area |
| `Meta+Ctrl+L`    | Grow master area   |
| `Meta+Ctrl+J`    | (reserved)         |
| `Meta+Ctrl+K`    | (reserved)         |

## Layout

Switch the focused cell's layout.

| Shortcut         | Layout       |
| ---------------- | ------------ |
| `Meta+Shift+T`   | tile         |
| `Meta+M`         | monocle      |
| `Meta+C`         | columns      |
| `Meta+S`         | spiral       |
| `Meta+Shift+D`   | threecolumn  |
| `Meta+F`         | floating     |

`Meta+T` (Show Desktop) and `Meta+D` (Minimize All) are KDE defaults that
KGlobalAccel silently drops on collision, so tile and threecolumn use the
`Shift` variants.

## Master area

| Shortcut             | Action                  |
| -------------------- | ----------------------- |
| `Meta+Shift+-`       | Shrink master area      |
| `Meta+Shift+=`       | Grow master area        |
| `Meta+Shift+,`       | Fewer master windows    |
| `Meta+Shift+.`       | More master windows     |

`Meta+Minus`/`Meta+Equal` are KDE's desktop "Zoom Out/In", so the master-ratio
shortcuts use the `Shift` variants.
