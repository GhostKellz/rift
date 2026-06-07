// rift-kwin entry point.
//
// This module is intentionally thin: it forwards events and applies geometry,
// holding no layout logic. For M1 it performs only the handshake — send a
// `Hello`, log the daemon's `Ack`. Event forwarding and geometry application
// arrive in M2/M3 once the in-KWin transport is settled (see tasks/spec.md).

import {
  GeometrySet,
  Hello,
  Keybinding,
  PROTOCOL_VERSION,
  Reply,
  Topology,
  topologyEvent,
} from "./protocol";
import { Transport } from "./transport";
import { DBusTransport } from "./dbus_transport";

/** Sentinel placement for windows pinned to all desktops/activities. */
const ALL = "all";

/**
 * Build a topology snapshot from the live KWin workspace.
 *
 * Windows on "all desktops"/"all activities" (empty arrays) are resolved to a
 * single sentinel tuple for now; richer handling lands with the cell model's
 * multi-placement support. Not exercised by the Node harness (no `workspace`
 * global there) — validated on a real session during M2 transport work.
 */
export function collectTopology(): Topology {
  return {
    outputs: workspace.screens.map((s) => ({
      id: s.name,
      name: s.name,
      rect: {
        x: s.geometry.x,
        y: s.geometry.y,
        width: s.geometry.width,
        height: s.geometry.height,
      },
    })),
    desktops: workspace.desktops.map((d) => ({ id: d.id, name: d.name })),
    activities: workspace.activities.map((a) => ({ id: a, name: a })),
    windows: workspace
      .windowList()
      .filter((w) => w.normalWindow && !w.skipTaskbar)
      .map((w) => ({
        id: w.internalId,
        output: w.output.name,
        desktop: w.desktops.length > 0 ? w.desktops[0].id : ALL,
        activity: w.activities.length > 0 ? w.activities[0] : ALL,
        class: w.resourceClass,
        title: w.caption,
      })),
  };
}

/**
 * Begin a session over the given transport: announce ourselves and react to
 * the daemon's reply. Pure protocol — no I/O or framing lives here.
 *
 * Returns a `resync` callback the caller drives on every liveness tick (window
 * events and the heartbeat). While the handshake is unacknowledged — the daemon
 * was down at load or has restarted — `resync` re-sends `Hello`; once acked it
 * pushes topology. D-Bus failures are silent, so retrying the handshake until an
 * `Ack` lands is what recovers a late or restarted daemon without a reload.
 */
export function start(transport: Transport, kwinVersion: string): () => void {
  const hello: Hello = {
    type: "Hello",
    kwin_version: kwinVersion,
    protocol: PROTOCOL_VERSION,
  };

  // Hello and Focus events both reply with `Ack`; only the first (the handshake
  // acknowledgement) should wire up shortcuts, focus reporting, and topology.
  let helloAcked = false;
  transport.onMessage((msg) => {
    const reply = msg as Reply;
    switch (reply.type) {
      case "Ack":
        if (!helloAcked) {
          helloAcked = true;
          print("[rift] daemon acknowledged hello");
          // The daemon owns the keybinding table (defaults overlaid by the
          // user's `[keys]`); fetch it and register each entry generically.
          transport.send({ type: "GetKeybindings" });
          reportFocus(transport);
          pushTopology(transport);
        }
        break;
      case "Keybindings":
        registerKeybindings(transport, reply.bindings);
        break;
      case "Reconciled":
        print(`[rift] reconciled: ${reply.cells} cells, ${reply.windows} windows`);
        break;
      case "Geometry":
        applyGeometry(reply);
        break;
      case "GeometryResync":
        // A cross-output move: apply the geometry (which physically relocates
        // the window), then re-push topology so the daemon re-keys it on its
        // new output from KWin's updated state.
        applyGeometry(reply);
        pushTopology(transport);
        break;
      case "Focus":
        focusWindow(reply.window);
        break;
      case "Error":
        print(`[rift] daemon error: ${reply.message}`);
        break;
      default:
        print(`[rift] unexpected reply: ${reply.type}`);
    }
  });

  transport.send(hello);

  return () => {
    if (helloAcked) {
      pushTopology(transport);
    } else {
      transport.send(hello);
    }
  };
}

/**
 * Register the daemon-supplied shortcuts, each forwarding its command to the
 * daemon. The daemon is the single source of truth for the table (defaults plus
 * the user's `riftrc [keys]` overrides); the script holds no binding list of its
 * own. KWin keys overrides by the binding `id`, so user rebinds in System
 * Settings survive script reloads — the daemon keeps ids stable for that reason.
 */
export function registerKeybindings(
  transport: Transport,
  bindings: Keybinding[],
): void {
  for (const b of bindings) {
    registerShortcut(b.id, b.description, b.key, () => transport.send(b.command));
  }
  print(`[rift] registered ${bindings.length} shortcuts`);
}

/**
 * Forward active-window changes to the daemon so it can track focus. KWin
 * passes null when focus is lost; that maps to a `Focus` event with no window.
 */
export function reportFocus(transport: Transport): void {
  workspace.windowActivated.connect((win) => {
    transport.send({ type: "Focus", window: win ? win.internalId : null });
  });
}

/** Move keyboard focus to the daemon-chosen window, matched by `internalId`. */
export function focusWindow(id: string | null): void {
  if (id === null) return;
  for (const w of workspace.windowList()) {
    if (w.internalId === id) {
      workspace.activeWindow = w;
      return;
    }
  }
}

/** Collect the current topology and forward it to the daemon. */
export function pushTopology(transport: Transport): void {
  transport.send(topologyEvent(collectTopology()));
}

/**
 * Apply a batch of computed window geometries to the live workspace.
 *
 * Windows are matched by `internalId`; the daemon-supplied rectangle is written
 * to `frameGeometry` in one pass so a topology change yields a single coherent
 * relayout. Geometries referencing windows no longer present are ignored.
 */
export function applyGeometry(set: GeometrySet): void {
  const byId = new Map<string, KWinWindow>();
  for (const w of workspace.windowList()) {
    byId.set(w.internalId, w);
  }
  let applied = 0;
  for (const g of set.windows) {
    const win = byId.get(g.id);
    if (!win) continue;
    win.frameGeometry = {
      x: g.rect.x,
      y: g.rect.y,
      width: g.rect.width,
      height: g.rect.height,
    };
    applied++;
  }
  print(`[rift] applied geometry to ${applied}/${set.windows.length} windows`);
}

/**
 * Timers are kept referenced at module scope so the QJSEngine garbage collector
 * does not reclaim them out from under their live signal connections.
 */
const liveTimers: KWinTimer[] = [];

/** How often to re-push topology as a liveness net (callDBus errors are silent). */
const RESYNC_INTERVAL_MS = 5000;

/**
 * Live-session entry: stand up the D-Bus transport, run the handshake, and keep
 * the daemon's view of the topology fresh. Re-push on every compositor change
 * that can affect tiling, plus a periodic heartbeat so a restarted or late
 * daemon recovers without reloading the script.
 */
function runInKwin(): void {
  const transport = new DBusTransport();
  // `resync` re-sends Hello until the daemon acks, then pushes topology — so a
  // daemon that was down at load (or restarts) recovers on the next tick.
  const resync = start(transport, "plasma-6");
  workspace.windowAdded.connect(resync);
  workspace.windowRemoved.connect(resync);
  workspace.desktopsChanged.connect(resync);
  workspace.currentDesktopChanged.connect(resync);
  workspace.currentActivityChanged.connect(resync);
  workspace.activitiesChanged.connect(resync);
  workspace.virtualScreenGeometryChanged.connect(resync);
  workspace.screensChanged.connect(resync);

  const timer = new QTimer();
  timer.interval = RESYNC_INTERVAL_MS;
  timer.singleShot = false;
  timer.timeout.connect(resync);
  timer.start();
  liveTimers.push(timer);

  print("[rift] rift-kwin live (D-Bus transport)");
}

// Only run inside KWin, where `workspace` is defined; importing this module
// elsewhere (e.g. tooling) is a no-op.
if (typeof workspace !== "undefined") {
  runInKwin();
}
