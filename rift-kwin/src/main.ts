// rift-kwin entry point.
//
// This module is intentionally thin: it forwards events and applies geometry,
// holding no layout logic. For M1 it performs only the handshake — send a
// `Hello`, log the daemon's `Ack`. Event forwarding and geometry application
// arrive in M2/M3 once the in-KWin transport is settled (see tasks/spec.md).

import {
  Command,
  Direction,
  GeometrySet,
  Hello,
  LayoutKind,
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
      })),
  };
}

/**
 * Begin a session over the given transport: announce ourselves and react to
 * the daemon's reply. Pure protocol — no I/O or framing lives here.
 */
export function start(transport: Transport, kwinVersion: string): void {
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
          registerKeybindings(transport);
          reportFocus(transport);
          pushTopology(transport);
        }
        break;
      case "Reconciled":
        print(`[rift] reconciled: ${reply.cells} cells, ${reply.windows} windows`);
        break;
      case "Geometry":
        applyGeometry(reply);
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

  const hello: Hello = {
    type: "Hello",
    kwin_version: kwinVersion,
    protocol: PROTOCOL_VERSION,
  };
  transport.send(hello);
}

/** Default keybindings: command sent to the daemon for each shortcut. */
interface Binding {
  /** Stable identifier KWin uses to key the (user-overridable) shortcut. */
  id: string;
  /** Human-readable description shown in System Settings. */
  text: string;
  /** Default key sequence (the user may rebind it). */
  key: string;
  /** Command forwarded to the daemon when the shortcut fires. */
  command: Command;
}

/** vim-style focus/move plus layout and master-area controls. */
const BINDINGS: Binding[] = [
  focusBind("h", "Left"),
  focusBind("j", "Down"),
  focusBind("k", "Up"),
  focusBind("l", "Right"),
  moveBind("h", "Left"),
  moveBind("j", "Down"),
  moveBind("k", "Up"),
  moveBind("l", "Right"),
  // Tile (Meta+T) and ThreeColumn (Meta+D) collide with KDE defaults
  // (Show Desktop et al.), which KGlobalAccel silently drops — use Shift.
  layoutBind("t", "Tile", "Meta+Shift+T"),
  layoutBind("m", "Monocle"),
  layoutBind("c", "Columns"),
  layoutBind("s", "Spiral"),
  layoutBind("d", "ThreeColumn", "Meta+Shift+D"),
  layoutBind("f", "Floating"),
  {
    id: "rift_toggle_tiling",
    text: "Rift: Toggle auto-tiling",
    key: "Meta+Y",
    command: { type: "ToggleTiling" },
  },
  {
    // Meta+G is KDE's "Toggle Grid View"; Meta+Shift+Space is free and matches
    // the i3/sway float-toggle convention.
    id: "rift_toggle_float",
    text: "Rift: Toggle float (focused)",
    key: "Meta+Shift+Space",
    command: { type: "ToggleFloat", window: null },
  },
  {
    id: "rift_master_ratio_dec",
    text: "Rift: Shrink master area",
    key: "Meta+Minus",
    command: { type: "MasterRatio", delta: -0.05 },
  },
  {
    id: "rift_master_ratio_inc",
    text: "Rift: Grow master area",
    key: "Meta+Equal",
    command: { type: "MasterRatio", delta: 0.05 },
  },
  {
    id: "rift_master_count_dec",
    text: "Rift: Fewer master windows",
    key: "Meta+Shift+Comma",
    command: { type: "MasterCount", delta: -1 },
  },
  {
    id: "rift_master_count_inc",
    text: "Rift: More master windows",
    key: "Meta+Shift+Period",
    command: { type: "MasterCount", delta: 1 },
  },
];

function focusBind(letter: string, direction: Direction): Binding {
  return {
    id: `rift_focus_${direction.toLowerCase()}`,
    text: `Rift: Focus ${direction}`,
    key: `Meta+${letter.toUpperCase()}`,
    command: { type: "Focus", direction },
  };
}

function moveBind(letter: string, direction: Direction): Binding {
  return {
    id: `rift_move_${direction.toLowerCase()}`,
    text: `Rift: Move window ${direction}`,
    key: `Meta+Shift+${letter.toUpperCase()}`,
    command: { type: "Move", direction },
  };
}

function layoutBind(letter: string, layout: LayoutKind, key?: string): Binding {
  return {
    id: `rift_layout_${layout.toLowerCase()}`,
    text: `Rift: ${layout} layout`,
    key: key ?? `Meta+${letter.toUpperCase()}`,
    command: { type: "SetLayout", layout },
  };
}

/**
 * Register the default shortcuts, each forwarding its command to the daemon.
 * KWin keys overrides by the binding `id`, so user rebinds in System Settings
 * survive script reloads.
 */
export function registerKeybindings(transport: Transport): void {
  for (const b of BINDINGS) {
    registerShortcut(b.id, b.text, b.key, () => transport.send(b.command));
  }
  print(`[rift] registered ${BINDINGS.length} shortcuts`);
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
  start(transport, "plasma-6");

  const resync = () => pushTopology(transport);
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
