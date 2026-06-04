// In-KWin transport over D-Bus.
//
// A KWin script cannot open sockets; its only IPC is the outbound, async
// `callDBus`. Each `send` issues a `Dispatch` call carrying the JSON-encoded
// message and routes the JSON reply back through the registered `onMessage`
// handler — preserving the exact request/reply flow the Node harness exercises
// over a socket. There are no unprompted pushes from the daemon, so a
// request/reply method suffices.

import { Transport } from "./transport";

/** Bus name, object path, and interface the daemon serves (mirrors dbus.rs). */
export const SERVICE = "dev.ghostkellz.Rift";
export const PATH = "/dev/ghostkellz/Rift";
export const INTERFACE = "dev.ghostkellz.Rift";

export class DBusTransport implements Transport {
  private handler: ((msg: unknown) => void) | null = null;

  send(msg: unknown): void {
    callDBus(
      SERVICE,
      PATH,
      INTERFACE,
      "Dispatch",
      JSON.stringify(msg),
      (reply: unknown) => {
        // KWin drops the callback entirely on D-Bus error, so a missing or
        // non-string reply just means "no answer" — ignore it; the resync
        // heartbeat recovers a restarted daemon.
        if (typeof reply !== "string" || this.handler === null) return;
        let parsed: unknown;
        try {
          parsed = JSON.parse(reply);
        } catch (e) {
          print(`[rift] dropping unparseable reply: ${e}`);
          return;
        }
        this.handler(parsed);
      },
    );
  }

  onMessage(cb: (msg: unknown) => void): void {
    this.handler = cb;
  }

  close(): void {
    // Nothing to tear down: callDBus holds no persistent connection.
    this.handler = null;
  }
}
