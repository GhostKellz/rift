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

| Shortcut  | Layout       |
| --------- | ------------ |
| `Meta+T`  | tile         |
| `Meta+M`  | monocle      |
| `Meta+C`  | columns      |
| `Meta+S`  | spiral       |
| `Meta+D`  | threecolumn  |
| `Meta+F`  | floating     |

## Master area

| Shortcut             | Action                  |
| -------------------- | ----------------------- |
| `Meta+Minus`         | Shrink master area      |
| `Meta+Equal`         | Grow master area        |
| `Meta+Shift+Comma`   | Fewer master windows    |
| `Meta+Shift+Period`  | More master windows     |
