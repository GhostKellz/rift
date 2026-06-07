# The riftrc File

Rift reads a single TOML file. It is optional: with no file present, the daemon
runs with built-in defaults.

## Location

Resolved in this order:

1. `$XDG_CONFIG_HOME/riftrc`
2. `$HOME/.config/riftrc`

The daemon never creates this file or its parent directory.

## Format

Three optional sections plus an optional `[keys]` table and `[[rules]]` array.
Omitted sections fall back to defaults; unknown keys are rejected so typos surface
instead of being silently ignored.

```toml
[layout]
default = "tile"
master_ratio = 0.6
master_count = 1

[gaps]
inner = 8
outer = 12

[behavior]
per_desktop = true
per_activity = false
focus_follows_mouse = false

[keys]
rift_layout_monocle = "Meta+Shift+M"

[[rules]]
class = "org.kde.polkit-kde-authentication-agent-1"
float = true
```

The `[keys]` table is optional. Each entry maps a built-in binding id to a
replacement key sequence; the daemon applies it before handing the table to the
script. Unknown ids and empty sequences are rejected. See
[reference.md](reference.md) for the binding ids and the full rule shape.

The `[[rules]]` array is optional and may repeat. Each entry floats windows whose
class or title matches (case-sensitive substring); see
[reference.md](reference.md) for the full rule shape.

See [reference.md](reference.md) for every key, its default, and its valid range.

## Validation

The file is validated as a whole on load. An invalid file is **rejected entirely**
with a diagnostic — it is never partially applied. On a failed `Reload`, the daemon
keeps the previously loaded configuration.

## Live reload

The daemon watches the config file's parent directory and reloads on change. It
watches the directory rather than the file so it survives the replace-on-save
pattern editors use (write to a temporary file, then rename over the target). A
reload can also be forced with `riftctl reload`.

When a reload changes layout defaults or parameters, the new values take effect on
the next reconcile or geometry pass; existing cells are not retroactively rebuilt.
