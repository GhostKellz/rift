// Reconnect harness: prove the script recovers a daemon that is down at load or
// restarts. `start()` returns a `resync` callback that re-sends `Hello` until the
// daemon answers with `Ack`, then switches to pushing `Topology`. The one-time,
// Ack-gated setup (request the keybinding table, focus reporting, topology) must
// fire whenever the Ack finally arrives — closing the down-at-load and restart
// cases without a script reload. Shortcuts register only once the daemon answers
// the resulting `GetKeybindings` with its `Keybindings` table.
//
// The KWin globals (`print`, `registerShortcut`, `workspace`) are stubbed on
// `globalThis`; a mock Transport stands in for the D-Bus seam so we can withhold
// the `Ack` and inspect exactly what the script sends.

import { test } from "node:test";
import assert from "node:assert/strict";

import { start } from "../dist/main.mjs";

/**
 * Install the KWin globals `start()` and its callbacks touch. Returns the
 * `shortcuts` sink so a test can assert keybindings registered. `workspace` is
 * the minimum shape `collectTopology`/`reportFocus` read.
 */
function installKwinGlobals() {
  const shortcuts = [];
  globalThis.print = () => {};
  globalThis.registerShortcut = (id, text, key, cb) => {
    shortcuts.push({ id, text, key, cb });
  };
  globalThis.workspace = {
    screens: [{ name: "DP-1", geometry: { x: 0, y: 0, width: 1920, height: 1080 } }],
    desktops: [{ id: "d1", name: "Desktop 1" }],
    activities: ["a1"],
    windowList: () => [
      {
        internalId: "w1",
        normalWindow: true,
        skipTaskbar: false,
        resourceClass: "konsole",
        caption: "Terminal",
        output: { name: "DP-1" },
        desktops: [{ id: "d1" }],
        activities: ["a1"],
      },
    ],
    windowActivated: { connect: () => {} },
  };
  return shortcuts;
}

/** A minimal `Keybindings` reply, as the daemon answers `GetKeybindings`. */
function keybindingsReply() {
  return {
    type: "Keybindings",
    bindings: [
      {
        id: "rift_focus_left",
        description: "Rift: Focus Left",
        key: "Meta+H",
        command: { type: "Focus", direction: "Left" },
      },
      {
        id: "rift_master_ratio_dec",
        description: "Rift: Shrink master area",
        key: "Meta+Shift+-",
        command: { type: "MasterRatio", delta: -0.05 },
      },
    ],
  };
}

/** A Transport that records sends and lets the test deliver replies on demand. */
function mockTransport() {
  const sent = [];
  let handler = null;
  return {
    sent,
    send: (msg) => sent.push(msg),
    onMessage: (cb) => {
      handler = cb;
    },
    close: () => {
      handler = null;
    },
    deliver: (msg) => {
      if (handler) handler(msg);
    },
  };
}

test("resync re-sends Hello while unacked, then pushes Topology after Ack", () => {
  const shortcuts = installKwinGlobals();
  const t = mockTransport();

  // start() announces itself once.
  const resync = start(t, "plasma-6");
  assert.equal(t.sent.length, 1);
  assert.equal(t.sent[0].type, "Hello");

  // While the daemon is silent (down at load), every heartbeat retries Hello —
  // never the Ack-gated setup.
  resync();
  resync();
  assert.equal(t.sent.length, 3);
  assert.ok(t.sent.every((m) => m.type === "Hello"));
  assert.equal(shortcuts.length, 0, "no keybindings before the daemon acks");

  // The daemon comes up and acks: the script requests the keybinding table and
  // pushes topology, but registers nothing until the table arrives.
  t.deliver({ type: "Ack" });
  assert.ok(
    t.sent.some((m) => m.type === "GetKeybindings"),
    "the handshake requests the daemon-owned keybinding table",
  );
  assert.equal(shortcuts.length, 0, "no shortcuts until the table is delivered");
  assert.equal(t.sent[t.sent.length - 1].type, "Topology");

  // The daemon answers with its table; now the shortcuts register, one per entry.
  const table = keybindingsReply();
  t.deliver(table);
  assert.equal(
    shortcuts.length,
    table.bindings.length,
    "one shortcut per daemon-supplied binding",
  );
  assert.deepEqual(
    shortcuts.map((s) => s.key),
    table.bindings.map((b) => b.key),
    "shortcuts register with the daemon-supplied keys (glyphs intact)",
  );

  // Now acked, resync switches to topology pushes instead of re-handshaking.
  resync();
  assert.equal(t.sent[t.sent.length - 1].type, "Topology");
  assert.ok(
    t.sent.filter((m) => m.type === "Hello").length === 3,
    "no further Hello after the Ack",
  );
});

test("a second Ack does not re-request the keybinding table (Focus replies also Ack)", () => {
  installKwinGlobals();
  const t = mockTransport();

  const resync = start(t, "plasma-6");
  t.deliver({ type: "Ack" });
  const requests = () => t.sent.filter((m) => m.type === "GetKeybindings").length;
  assert.equal(requests(), 1, "the handshake requests the table once");

  // Focus events reply with Ack too; the handshake guard keeps setup idempotent.
  t.deliver({ type: "Ack" });
  assert.equal(requests(), 1, "a later Ack does not re-request the table");

  // resync stays in topology mode.
  resync();
  assert.equal(t.sent[t.sent.length - 1].type, "Topology");
});
