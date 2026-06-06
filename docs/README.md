# Rift Documentation

Rift is a dynamic tiling window manager for KDE Plasma 6 on Wayland. It runs as a
standalone daemon (`riftd`) that owns all layout logic, a CLI (`riftctl`) for
control and inspection, and a thin KWin script (`rift-kwin`) that forwards
compositor events to the daemon and applies the geometry it computes.

## Architecture

- [architecture/overview.md](architecture/overview.md) — components and data flow.
- [architecture/cell-model.md](architecture/cell-model.md) — the output × desktop ×
  activity cell model and reconcile.
- [architecture/ipc-protocol.md](architecture/ipc-protocol.md) — framing and the
  `Event`/`Command`/`Reply` wire types.
- [architecture/transport.md](architecture/transport.md) — the in-KWin D-Bus
  transport and the Unix socket.
- [architecture/compositor-support.md](architecture/compositor-support.md) — the
  adapter model and Wayland compositor support (KWin, wlroots).

## Configuration

- [config/riftrc.md](config/riftrc.md) — file location, format, and live reload.
- [config/reference.md](config/reference.md) — every option with defaults and bounds.

## Layouts

- [layouts/overview.md](layouts/overview.md) — the six built-in layouts.

## Usage

- [usage/keybindings.md](usage/keybindings.md) — default shortcuts.
- [usage/riftctl.md](usage/riftctl.md) — the command-line client.
- [usage/pager.md](usage/pager.md) — the numbered virtual-desktop pager plasmoid.

## Operating

- [troubleshooting/diagnostics.md](troubleshooting/diagnostics.md) — when a session
  isn't tiling.
- [building.md](building.md) — building from source and installing the KWin script.

## Project layout

| Path              | What it is                                                    |
| ----------------- | ------------------------------------------------------------- |
| `crates/riftd`    | The daemon: layout engine, cell model, reconcile, IPC server. |
| `crates/riftctl`  | The command-line client.                                      |
| `crates/rift-ipc` | Shared protocol: framing and `Event`/`Command`/`Reply` types. |
| `rift-kwin`       | The KWin script (TypeScript), built to a single JS bundle.    |
| `rift-pager`      | The virtual-desktop pager plasmoid (QML, daemon-independent).  |
