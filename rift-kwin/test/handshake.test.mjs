// Handshake harness: prove a JS client speaks Rift's wire protocol against a
// real `riftd`. This stands in for the in-KWin transport (an open question for
// M2) and validates the framing in ../src/protocol.ts end to end.

import { test } from "node:test";
import assert from "node:assert/strict";
import net from "node:net";
import { spawn } from "node:child_process";
import { mkdtemp, rm, access, writeFile, rename } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  encode,
  FrameDecoder,
  PROTOCOL_VERSION,
  topologyEvent,
} from "../dist/protocol.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = join(here, "..", "..");

function riftdBinary() {
  if (process.env.RIFTD_BIN) return process.env.RIFTD_BIN;
  return [
    join(repoRoot, "target", "debug", "riftd"),
    join(repoRoot, "target", "x86_64-unknown-linux-gnu", "debug", "riftd"),
  ];
}

async function firstExisting(paths) {
  for (const p of [].concat(paths)) {
    try {
      await access(p);
      return p;
    } catch {
      /* keep looking */
    }
  }
  throw new Error(
    `riftd binary not found; build it first (cargo build). Looked in: ${[]
      .concat(paths)
      .join(", ")}`,
  );
}

function waitFor(predicate, { tries = 200, intervalMs = 10 } = {}) {
  return new Promise((resolve, reject) => {
    let n = 0;
    const tick = async () => {
      try {
        if (await predicate()) return resolve();
      } catch {
        /* retry */
      }
      if (++n >= tries) return reject(new Error("timed out waiting"));
      setTimeout(tick, intervalMs);
    };
    tick();
  });
}

/**
 * Spawn riftd on a throwaway runtime dir and connect a framed client to it.
 * Returns helpers for request/reply plus a teardown function.
 */
async function connectDaemon({ config } = {}) {
  const bin = await firstExisting(riftdBinary());
  const runtimeDir = await mkdtemp(join(tmpdir(), "rift-test-"));
  const socketPath = join(runtimeDir, "rift", "rift.sock");

  // The daemon resolves its config from $XDG_CONFIG_HOME/riftrc; seed one when
  // the test supplies config text so reload behavior can be exercised.
  const configHome = await mkdtemp(join(tmpdir(), "rift-cfg-"));
  const configPath = join(configHome, "riftrc");
  if (config !== undefined) {
    await writeFile(configPath, config);
  }

  const daemon = spawn(bin, [], {
    env: { ...process.env, XDG_RUNTIME_DIR: runtimeDir, XDG_CONFIG_HOME: configHome },
    stdio: "ignore",
  });

  await waitFor(() => access(socketPath).then(() => true));
  const socket = net.createConnection({ path: socketPath });
  await new Promise((resolve, reject) => {
    socket.once("connect", resolve);
    socket.once("error", reject);
  });

  // Decode incoming frames into a queue, handing them out on demand.
  const decoder = new FrameDecoder();
  const pending = [];
  const waiters = [];
  socket.on("data", (chunk) => {
    for (const msg of decoder.push(new Uint8Array(chunk))) {
      const waiter = waiters.shift();
      if (waiter) waiter(msg);
      else pending.push(msg);
    }
  });

  const send = (msg) => socket.write(encode(msg));
  const recv = () =>
    new Promise((resolve) => {
      const queued = pending.shift();
      if (queued) resolve(queued);
      else waiters.push(resolve);
    });
  const request = async (msg) => {
    send(msg);
    return recv();
  };

  const teardown = async () => {
    socket.end();
    daemon.kill("SIGTERM");
    await rm(runtimeDir, { recursive: true, force: true });
    await rm(configHome, { recursive: true, force: true });
  };

  return { request, teardown, configPath };
}

test("hello is acknowledged by riftd", async () => {
  const { request, teardown } = await connectDaemon();
  try {
    const reply = await request({
      type: "Hello",
      kwin_version: "test-6.2.0",
      protocol: PROTOCOL_VERSION,
    });
    assert.deepEqual(reply, { type: "Ack" });
  } finally {
    await teardown();
  }
});

test("topology snapshot yields window geometry", async () => {
  const { request, teardown } = await connectDaemon();
  try {
    const topology = topologyEvent({
      outputs: [
        { id: "DP-1", name: "DP-1", rect: { x: 0, y: 0, width: 1920, height: 1080 } },
      ],
      desktops: [{ id: "d1", name: "Desktop 1" }],
      activities: [{ id: "a1", name: "Default" }],
      windows: [
        { id: "w1", output: "DP-1", desktop: "d1", activity: "a1" },
        { id: "w2", output: "DP-1", desktop: "d1", activity: "a1" },
      ],
    });
    const reply = await request(topology);
    assert.equal(reply.type, "Geometry");
    assert.equal(reply.windows.length, 2);
    // The default tile places the master (w1) left of the stack (w2).
    const [w1, w2] = reply.windows;
    assert.equal(w1.id, "w1");
    assert.equal(w2.id, "w2");
    assert.ok(w1.rect.width > 0 && w1.rect.height > 0);
    assert.ok(w2.rect.x >= w1.rect.x + w1.rect.width);
  } finally {
    await teardown();
  }
});

// Rewrite the config atomically (temp file + rename), matching how real editors
// save. The daemon's watcher deliberately watches the parent directory for this
// reason; a plain in-place truncate would expose a transient empty file.
async function atomicWrite(path, content) {
  const tmp = `${path}.tmp`;
  await writeFile(tmp, content);
  await rename(tmp, path);
}

test("config is loaded from disk and reload reflects rewrites", async () => {
  const { request, teardown, configPath } = await connectDaemon({
    config: "[gaps]\ninner = 20\nouter = 30\n",
  });
  try {
    // GetConfig reflects the seeded riftrc.
    const loaded = await request({ type: "GetConfig" });
    assert.equal(loaded.type, "Config");
    assert.equal(loaded.loaded, true);
    assert.equal(loaded.gaps_inner, 20);
    assert.equal(loaded.gaps_outer, 30);

    // Rewrite the file and force a reload: the new values take effect.
    await atomicWrite(configPath, "[gaps]\ninner = 4\nouter = 4\n");
    const reloaded = await request({ type: "Reload" });
    assert.equal(reloaded.type, "Config");
    assert.equal(reloaded.gaps_inner, 4);
    assert.equal(reloaded.gaps_outer, 4);

    // An invalid rewrite is rejected; the prior config is retained.
    await atomicWrite(configPath, "[layout]\nmaster_ratio = 9.0\n");
    const rejected = await request({ type: "Reload" });
    assert.equal(rejected.type, "Error");
    const stillFour = await request({ type: "GetConfig" });
    assert.equal(stillFour.gaps_inner, 4);
  } finally {
    await teardown();
  }
});

test("focus event drives directional focus and layout control", async () => {
  const { request, teardown } = await connectDaemon();
  try {
    await request(
      topologyEvent({
        outputs: [
          { id: "DP-1", name: "DP-1", rect: { x: 0, y: 0, width: 1920, height: 1080 } },
        ],
        desktops: [{ id: "d1", name: "Desktop 1" }],
        activities: [{ id: "a1", name: "Default" }],
        windows: [
          { id: "w1", output: "DP-1", desktop: "d1", activity: "a1" },
          { id: "w2", output: "DP-1", desktop: "d1", activity: "a1" },
        ],
      }),
    );

    // The script reports the active window; the daemon tracks it.
    const ack = await request({ type: "Focus", window: "w1" });
    assert.deepEqual(ack, { type: "Ack" });

    // From the master (w1), the right neighbor is the stack window (w2).
    const focus = await request({ type: "Focus", direction: "Right" });
    assert.deepEqual(focus, { type: "Focus", window: "w2" });

    // Switching layout relays out the same two windows.
    const geom = await request({ type: "SetLayout", layout: "Monocle" });
    assert.equal(geom.type, "Geometry");
    assert.equal(geom.windows.length, 2);
  } finally {
    await teardown();
  }
});
