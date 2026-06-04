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

## Inspecting the effective config

`riftctl config` prints the values currently in effect, the resolved source path,
and whether a file was actually loaded.
