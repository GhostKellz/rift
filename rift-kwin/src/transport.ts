// Transport abstraction between the KWin script and the daemon.
//
// The script holds no layout logic and no knowledge of the wire framing beyond
// this seam. The concrete in-KWin transport (D-Bus `callDBus` vs a Qt socket
// helper) is an open question tracked for M2; see tasks/spec.md. Until then the
// only implementation lives in the Node handshake harness.

export interface Transport {
  /** Send a protocol message to the daemon. */
  send(msg: unknown): void;
  /** Register a handler for messages received from the daemon. */
  onMessage(cb: (msg: unknown) => void): void;
  /** Tear down the connection. */
  close(): void;
}
