# Compositor Support

Rift is built for Wayland. The daemon is compositor-agnostic — it computes layout
from topology and never talks to a compositor directly. Each compositor is reached
through a thin **adapter** that satisfies one contract, so supporting a new
compositor means writing an adapter, not touching the layout engine.

Broad Wayland compatibility — KWin first, wlroots-based compositors as a first-class
goal — is an explicit design target.

## The adapter contract

An adapter is responsible for four things, all expressed in the shared protocol:

1. **Collect topology** — enumerate outputs, desktops/workspaces, activities (where
   applicable), and managed windows, and send a `Topology` event.
2. **Apply geometry** — take the daemon's `GeometrySet` reply and position each
   window accordingly.
3. **Forward control** — translate user input (keybindings) into `Command`s and
   forward focus changes as `Focus` events.
4. **Stay live** — re-push topology on the compositor's relevant change signals so
   the daemon's view never drifts.

Anything that satisfies this contract can drive rift; the core does not care how.

## KWin (current)

The KWin adapter is an in-process script. It reads the live `workspace` and writes
`frameGeometry`, communicating with the daemon over D-Bus because the script
sandbox cannot open sockets (see [transport.md](transport.md)). This works because
the adapter runs *inside* the compositor and can position windows directly.

## wlroots-based compositors (goal)

wlroots compositors do not run KWin scripts and expose control differently. The
realistic integration paths:

- **Sway** — the Sway/i3 IPC socket (`SWAYSOCK`): subscribe to window/workspace
  events for topology, issue `move`/`resize`/`focus` commands to apply geometry.
  Note Sway is itself a tiler, so an adapter must drive it in a manual/floating mode
  it can fully position.
- **Hyprland** — the Hyprland IPC socket (`hyprctl` / event socket), or a native
  plugin for tighter control.
- **river** — river's layout protocol, where an external layout generator is the
  intended extension point.

### The constraint to be honest about

On Wayland, an ordinary external client generally **cannot** position another
client's windows — that is a deliberate part of the security model. External
control therefore depends on a compositor-specific channel (an IPC the compositor
chooses to expose, an in-process script, or a plugin). This is exactly why rift
splits "compute geometry" (portable, in the daemon) from "apply geometry"
(per-compositor, in the adapter): the portable half is written once, and each
adapter does only what its compositor actually permits.

## Status

| Compositor          | Adapter        | Status   |
| ------------------- | -------------- | -------- |
| KWin (Plasma 6)     | in-proc script | Working  |
| Sway / wlroots      | IPC client     | Planned  |
| Hyprland            | IPC / plugin   | Planned  |
| river               | layout proto   | Planned  |
