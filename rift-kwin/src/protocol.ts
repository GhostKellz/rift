// Rift IPC protocol: message shapes and length-prefixed JSON framing.
//
// This mirrors the Rust `rift-ipc` crate. The wire format is a 4-byte
// big-endian length prefix followed by a JSON body. Keep the two in sync.

export const PROTOCOL_VERSION = 1;

/** Maximum accepted frame body size (1 MiB), matching `rift-ipc::MAX_FRAME`. */
export const MAX_FRAME = 1 << 20;

/** Handshake the script sends to the daemon on connect. */
export interface Hello {
  type: "Hello";
  kwin_version: string;
  protocol: number;
}

/** An axis-aligned rectangle in global compositor pixel coordinates. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Output {
  id: string;
  name: string;
  rect: Rect;
}

export interface Desktop {
  id: string;
  name: string;
}

export interface Activity {
  id: string;
  name: string;
}

export interface Window {
  id: string;
  output: string;
  desktop: string;
  activity: string;
}

/** Full snapshot of the live KWin topology, mirroring `rift_ipc::Topology`. */
export interface Topology {
  outputs: Output[];
  desktops: Desktop[];
  activities: Activity[];
  windows: Window[];
}

/** A topology snapshot event forwarded to the daemon. */
export type TopologyEvent = { type: "Topology" } & Topology;

/** The tiling layout assigned to a cell, mirroring `rift_ipc::LayoutKind`. */
export type LayoutKind =
  | "Tile"
  | "Monocle"
  | "Columns"
  | "Spiral"
  | "ThreeColumn"
  | "Floating";

/** A cardinal direction for focus and movement, mirroring `rift_ipc::Direction`. */
export type Direction = "Left" | "Right" | "Up" | "Down";

/** Notification that the active window changed (null when focus is lost). */
export type FocusEvent = { type: "Focus"; window: string | null };

/** Events the script pushes to the daemon. */
export type Event = Hello | TopologyEvent | FocusEvent;

/** Control/query messages a client sends to the daemon. */
export type Command =
  | { type: "Status" }
  | { type: "Reset" }
  | { type: "Focus"; direction: Direction }
  | { type: "Move"; direction: Direction }
  | { type: "SetLayout"; layout: LayoutKind }
  | { type: "MasterRatio"; delta: number }
  | { type: "MasterCount"; delta: number }
  | { type: "ToggleTiling" }
  | { type: "ToggleFloat"; window: string | null }
  | { type: "GetConfig" }
  | { type: "Reload" };

/** Result of a reconcile pass. */
export interface ReconcileReport {
  cells: number;
  windows: number;
}

/** Computed target geometry for a single managed window. */
export interface WindowGeometry {
  id: string;
  rect: Rect;
}

/** A batch of window geometries to apply in one relayout. */
export interface GeometrySet {
  windows: WindowGeometry[];
}

/** The daemon's effective configuration, mirroring `rift_ipc::ConfigReport`. */
export interface ConfigReport {
  layout: LayoutKind;
  master_ratio: number;
  master_count: number;
  gaps_inner: number;
  gaps_outer: number;
  per_desktop: boolean;
  per_activity: boolean;
  focus_follows_mouse: boolean;
  tiling_enabled: boolean;
  source: string;
  loaded: boolean;
}

/** Replies the daemon sends back. */
export type Reply =
  | { type: "Ack" }
  | { type: "Status"; version: string; protocol: number; uptime_secs: number; cells: number; windows: number }
  | { type: "Reconciled"; cells: number; windows: number }
  | { type: "Geometry"; windows: WindowGeometry[] }
  | { type: "Focus"; window: string | null }
  | ({ type: "Config" } & ConfigReport)
  | { type: "Error"; message: string };

/** Wrap a topology snapshot in its event envelope for the wire. */
export function topologyEvent(topology: Topology): TopologyEvent {
  return { type: "Topology", ...topology };
}

/** Serialize a message into a length-prefixed JSON frame. */
export function encode(msg: unknown): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify(msg));
  if (body.length > MAX_FRAME) {
    throw new Error(`frame too large: ${body.length} bytes (max ${MAX_FRAME})`);
  }
  const frame = new Uint8Array(4 + body.length);
  new DataView(frame.buffer).setUint32(0, body.length, false);
  frame.set(body, 4);
  return frame;
}

/**
 * Incremental decoder for length-prefixed JSON frames. Feed it raw chunks;
 * it returns any complete messages and retains partial bytes for the next call.
 */
export class FrameDecoder {
  private buf = new Uint8Array(0);

  push(chunk: Uint8Array): unknown[] {
    const merged = new Uint8Array(this.buf.length + chunk.length);
    merged.set(this.buf);
    merged.set(chunk, this.buf.length);
    this.buf = merged;

    const out: unknown[] = [];
    while (this.buf.length >= 4) {
      const len = new DataView(
        this.buf.buffer,
        this.buf.byteOffset,
        4,
      ).getUint32(0, false);
      if (len > MAX_FRAME) {
        throw new Error(`frame too large: ${len} bytes (max ${MAX_FRAME})`);
      }
      if (this.buf.length < 4 + len) break;
      const body = this.buf.subarray(4, 4 + len);
      out.push(JSON.parse(new TextDecoder().decode(body)));
      this.buf = this.buf.subarray(4 + len);
    }
    return out;
  }
}
