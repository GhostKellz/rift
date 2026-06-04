# Transports

The daemon serves the same protocol over two transports. Clients pick whichever
fits; the daemon routes both through the identical dispatch path.

## Unix socket

Used by `riftctl` and the Node test harness. The daemon binds a stream socket at
`$XDG_RUNTIME_DIR/rift/rift.sock` with owner-only permissions (directory `0700`,
socket `0600`) and cleans up a stale socket on start. Messages use the
length-prefixed framing from [ipc-protocol.md](ipc-protocol.md).

## In-KWin D-Bus

Used by the KWin script. A KWin script runs in a sandboxed QJSEngine that **cannot
open sockets**; its only outbound IPC is the asynchronous `callDBus(...)`. So the
daemon also exposes a D-Bus interface:

- Service:   `dev.ghostkellz.Rift`
- Path:      `/dev/ghostkellz/Rift`
- Interface: `dev.ghostkellz.Rift`
- Method:    `Dispatch(String) -> String`

`Dispatch` is a **JSON passthrough**: it carries the exact same JSON the socket
uses, parses it, calls `Daemon::dispatch`, and returns the serialized reply. There
is no second protocol to keep in sync. The script wraps this in a `DBusTransport`
that issues `Dispatch` calls and routes the string reply back through its message
handler — preserving the request/reply flow the socket harness exercises.

Because every daemon→script message is a reply to a script-initiated request, a
plain request/reply method is sufficient; no inbound D-Bus signals are needed.

## Graceful degradation

D-Bus is brought up **best-effort** after the socket is bound. If no session bus is
present (headless, CI), the daemon logs a warning and continues — the socket path
is unaffected, so `riftctl` and the harness still work. The KWin script, in turn,
treats a missing D-Bus reply as "no answer" and relies on a periodic topology
re-push to recover a daemon that started late or restarted.
