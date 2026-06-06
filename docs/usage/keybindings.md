# Keybindings

The KWin script registers these shortcuts through KWin's global accelerator, keyed
by a stable id. Because KWin keys overrides by that id, rebinding a shortcut in
**System Settings → Shortcuts** survives script reloads.

Every shortcut forwards a command to the daemon; the daemon computes the result and
the script applies it.

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
