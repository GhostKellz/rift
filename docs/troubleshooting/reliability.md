# Reliability

How Rift behaves across the events that routinely break a tiler: a daemon
restart, a momentary loss of outputs, and monitor hotplug. These are the recovery
contracts the daemon and script uphold; if a session misbehaves around one of
them, this page is the reference for what *should* happen.

## Daemon restart (re-handshake)

The KWin script re-pushes topology on every compositor change and on a periodic
heartbeat. While its `Hello` is unacknowledged — the daemon was down at load or
has since restarted — the heartbeat re-sends `Hello` instead of topology. So a
daemon that comes up late, or restarts mid-session, recovers within one heartbeat
without reloading the script. No manual step is required.

When the daemon restarts it has an empty cell map; the script's next topology push
rebuilds it. Focus and floating marks are derived from topology, so they re-settle
on the same push.

## Transient empty topology (suspend / resume)

On resume-from-suspend, KWin can momentarily report **zero outputs** while it
reconfigures displays. A naive reconcile would treat that as "every output went
away" and wipe the cell map, collapsing the layout.

The daemon guards against this: if a topology arrives with no outputs **but the
last topology had outputs**, the daemon holds its prior state and ignores the blip.
The next non-empty topology relayouts normally. This is bounded to exactly the
"had outputs, now none" case, so a genuine first-ever empty topology still no-ops
as expected.

The guard intentionally holds — rather than clears — while outputs are absent:
there is nothing to tile without an output, and the layout must survive until they
return.

## Monitor hotplug

Adding or removing a monitor changes the output set. KWin emits the geometry/screen
change, the script re-pushes topology, and the daemon reconciles:

- **Unplug:** cells on the vanished output are dropped; their windows are re-keyed
  onto whichever output KWin moves them to on the next topology push.
- **Plug in:** the new output's cells materialize on first use with the default
  layout.

A removal that briefly passes through a zero-output state is covered by the
transient-empty guard above.

## Quick checks

- `riftctl status` — cell and window counts; watch them across the event.
- KWin script logs (`[rift]`) in the journal — the handshake line confirms the
  daemon re-acknowledged the script after a restart.
