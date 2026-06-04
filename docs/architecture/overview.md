# Architecture Overview

Rift splits cleanly into a compositor-agnostic core and a thin compositor adapter.
All layout decisions live in the daemon; the KWin script holds no layout logic.

## Components

- **`riftd`** — the daemon. Maintains the cell model, runs the layout engine,
  reconciles topology changes, and serves clients over both a Unix socket and
  D-Bus.
- **`riftctl`** — a command-line client that speaks the same protocol over the
  Unix socket for status, control, and config inspection.
- **`rift-ipc`** — the shared crate defining the framing and the
  `Event`/`Command`/`Reply` types used by every transport.
- **`rift-kwin`** — the KWin script. Collects topology, forwards events, and writes
  back the geometry the daemon returns.

## Data flow

1. The KWin script builds a **topology** snapshot — outputs, desktops, activities,
   and managed windows — and sends it to the daemon.
2. The daemon **reconciles** that snapshot against its cell model: dead cells and
   window references are pruned, new cells are materialized, and new windows are
   placed.
3. The daemon runs the **layout engine** for each cell and replies with a
   `GeometrySet` — a per-window rectangle batch.
4. The script **applies** each rectangle to the matching window's `frameGeometry`
   in a single pass.

Control commands (focus, move, layout switch, master adjustments) follow the same
request/reply path and likewise reply with geometry or focus.

## Design properties

- **Single source of truth.** `Daemon::dispatch(serde_json::Value) -> Reply` is the
  one entry point; every transport routes through it, so the socket and D-Bus
  cannot diverge.
- **No I/O in the core.** The layout engine is pure geometry; reconcile is pure
  state. This keeps the logic unit-testable without a compositor.
- **The adapter is replaceable.** The KWin script is one adapter; the core is
  compositor-agnostic by design. Supporting other Wayland compositors (wlroots-based
  — Sway, Hyprland, river) means writing an adapter against their IPC, not changing
  the layout engine. See [compositor-support.md](compositor-support.md).
