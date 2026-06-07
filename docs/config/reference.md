# Configuration Reference

Every option, its default, and its valid range. All sections and keys are optional.

## `[layout]`

| Key            | Type   | Default | Range / values                                              |
| -------------- | ------ | ------- | ----------------------------------------------------------- |
| `default`      | string | `tile`  | `tile`, `monocle`, `columns`, `spiral`, `threecolumn`, `floating` |
| `master_ratio` | float  | `0.6`   | `0.05`–`0.95`                                               |
| `master_count` | int    | `1`     | `≥ 1`                                                       |

`default` sets the layout for newly materialized cells. `master_ratio` is the
fraction of the cell width given to the master area; `master_count` is how many
windows occupy it.

## `[gaps]`

| Key     | Type | Default | Range  |
| ------- | ---- | ------- | ------ |
| `inner` | int  | `8`     | `≥ 0`  |
| `outer` | int  | `12`    | `≥ 0`  |

`inner` is the gap between tiles; `outer` is the inset between the tiled area and
the screen edge.

## `[behavior]`

| Key                   | Type | Default | Notes                                  |
| --------------------- | ---- | ------- | -------------------------------------- |
| `per_desktop`         | bool | `true`  | Treat each desktop as its own cell.    |
| `per_activity`        | bool | `false` | Treat each activity as its own cell.   |
| `focus_follows_mouse` | bool | `false` | Parsed and stored; runtime effect TBD. |

Behavior flags are parsed, validated, stored, and surfaced via `riftctl config`.
Some runtime effects are not yet wired (see the project notes); the values are
authoritative for what the daemon reports today.

## `[keys]`

The daemon owns the keybinding table; `[keys]` overrides individual entries by
their stable id. Each value is a QKeySequence portable-text string (e.g.
`Meta+Shift+M`). Unknown ids and empty values are rejected.

```toml
[keys]
rift_layout_monocle = "Meta+Shift+M"
rift_toggle_float = "Meta+Space"
```

| Id group                                                              | Command            | Default keys                                    |
| --------------------------------------------------------------------- | ------------------ | ----------------------------------------------- |
| `rift_focus_{left,down,up,right}`                                     | Focus              | `Meta+{H,J,K,L}`                                |
| `rift_move_{left,down,up,right}`                                      | Move window        | `Meta+Shift+{H,J,K,L}`                          |
| `rift_resize_{left,down,up,right}`                                    | Resize master split | `Meta+Ctrl+{H,J,K,L}`                          |
| `rift_layout_{tile,monocle,columns,spiral,threecolumn,floating}`     | Set layout         | `Meta+Shift+T`, `Meta+M`, `Meta+C`, `Meta+S`, `Meta+Shift+D`, `Meta+F` |
| `rift_toggle_tiling`                                                  | Toggle auto-tiling | `Meta+Y`                                        |
| `rift_toggle_float`                                                   | Toggle float       | `Meta+Shift+Space`                              |
| `rift_master_ratio_{dec,inc}`                                        | Master ratio       | `Meta+Shift+-`, `Meta+Shift+=`                  |
| `rift_master_count_{dec,inc}`                                        | Master count       | `Meta+Shift+,`, `Meta+Shift+.`                  |

Punctuation must be the literal glyph (`-`, `=`, `,`, `.`): QKeySequence portable
text has no `Minus`/`Comma`-style names, so those parse to an unknown key and never
bind. Run `riftctl keys` to print the effective table.

## `[[rules]]`

Window rules float matching windows instead of tiling them — useful for dialogs,
pickers, and apps that misbehave when tiled. Each rule is a `[[rules]]` array entry.

| Key     | Type   | Default | Notes                                            |
| ------- | ------ | ------- | ------------------------------------------------ |
| `class` | string | —       | Substring match against the window resource class. |
| `title` | string | —       | Substring match against the window title.        |
| `float` | bool   | `false` | When `true`, matched windows are excluded from tiling. |

Matching is case-sensitive substring. A rule must set at least one of `class` or
`title` (a rule with neither is rejected). When both are set, both must match.
Matched float windows keep their own geometry and are skipped by every layout.

```toml
[[rules]]
class = "org.kde.polkit-kde-authentication-agent-1"
float = true

[[rules]]
title = "Picture-in-Picture"
float = true
```

## Inspecting the effective config

`riftctl config` prints the values currently in effect, the resolved source path,
and whether a file was actually loaded.
