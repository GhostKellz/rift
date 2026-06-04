# IPC Protocol

The protocol is defined once in `rift-ipc` and shared by every transport. It is a
JSON message protocol with a small length-prefixed framing for stream transports.

## Framing

For the Unix socket (a byte stream), each message is sent as:

```
[ 4-byte big-endian length ][ JSON body ]
```

The length prefix bounds the body before allocation: bodies larger than `MAX_FRAME`
(1 MiB) are rejected rather than buffered. D-Bus carries the same JSON as a single
string argument, so it needs no framing of its own.

## Message kinds

Three tagged enums, each serialized with a `"type"` discriminator:

- **`Event`** — sent by the compositor adapter: `Hello`, `Topology`, `Focus`.
- **`Command`** — sent by clients (`riftctl` or the script): `Status`, `Reset`,
  `Focus`, `Move`, `SetLayout`, `MasterRatio`, `MasterCount`, `GetConfig`, `Reload`.
- **`Reply`** — sent by the daemon: `Ack`, `Status`, `Reconciled`, `Geometry`,
  `Focus`, `Config`, `Error`.

## Request / reply pairs

Every message to the daemon yields exactly one reply; there are no unprompted
pushes. The notable pairings:

| Sent                 | Reply                                  |
| -------------------- | -------------------------------------- |
| `Hello`              | `Ack`                                  |
| `Topology`           | `Geometry` (per-window rectangles)     |
| `Focus { window }`   | `Ack` (reports active window)          |
| `Focus { direction}` | `Focus { window }` (new focus)         |
| `SetLayout` / master | `Geometry` (relaid-out windows)        |
| `Reset`              | `Reconciled` (cell/window counts)      |
| `GetConfig`/`Reload` | `Config` or `Error`                    |
| anything malformed   | `Error { message }`                    |

This one-reply-per-message property is what lets the D-Bus transport use a plain
request/reply method (see [transport.md](transport.md)).

## Wire vs. config casing

Layout names are PascalCase on the JSON wire (`Tile`, `ThreeColumn`) but lowercase
in `riftrc` (`tile`, `threecolumn`); `rift-ipc` provides the conversion.
