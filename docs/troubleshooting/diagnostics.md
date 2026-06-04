# Diagnostics

A checklist for a session that isn't tiling. Work top to bottom; each step rules
out one layer.

## Is the daemon running and on the bus?

- Confirm `riftd` is running.
- `riftctl status` should return version/protocol/uptime. If it errors, the socket
  isn't there — the daemon isn't running or couldn't bind
  `$XDG_RUNTIME_DIR/rift/rift.sock`.
- For in-KWin tiling, the daemon must own `dev.ghostkellz.Rift` on the **session**
  bus. If it logged a D-Bus warning at start, no session bus was available; the
  socket still works for `riftctl` but the KWin script has nothing to talk to.

## Is the KWin script loaded?

- Check that the script is installed and enabled in **System Settings → Window
  Management → KWin Scripts**.
- KWin script logs (prefixed `[rift]`) appear in the journal. Look for the handshake
  line confirming the daemon acknowledged the script's `Hello`.

## Are windows being seen?

- `riftctl status` reports live cell and window counts. Zero windows means the
  script's topology snapshot is empty or not reaching the daemon.
- The script only manages normal, non-taskbar-skipping windows; dialogs and docks
  are intentionally excluded.

## Did a config change break things?

- `riftctl config` shows the effective values and whether a file was loaded.
- If a recent `riftrc` edit was rejected, the daemon kept the prior config and
  `riftctl reload` will print the parse diagnostic.

## Recovery

- `riftctl reset` forces a full rebuild from the last topology.
- Restarting `riftd` while the script runs is safe: the script's periodic topology
  re-push re-syncs the daemon without reloading the script.
