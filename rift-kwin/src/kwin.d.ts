// Minimal ambient declarations for the KWin scripting globals used here.
// Expanded as the script grows. These mirror the KWin 6 scripting API closely
// enough to typecheck topology collection; they are validated against a live
// session as part of M2 transport work (see tasks/spec.md).

/** KWin's logging primitive, printed to the journal. */
declare function print(...args: unknown[]): void;

/**
 * Bind a global shortcut. KWin invokes `callback` when the user presses the
 * default `keySequence` (overridable in System Settings, keyed by `title`).
 */
declare function registerShortcut(
  title: string,
  text: string,
  keySequence: string,
  callback: () => void,
): void;

/**
 * Asynchronously call a D-Bus method. The trailing args become the method
 * arguments; if the final arg is a function it is taken as the reply callback,
 * invoked with the reply's out-values spread as positional arguments. On error
 * KWin only logs — the callback is never invoked (see scripting.cpp).
 */
declare function callDBus(
  service: string,
  path: string,
  iface: string,
  method: string,
  ...args: unknown[]
): void;

/** A Qt timer, exposed to scripts as a global constructor. */
interface KWinTimer {
  /** Fire interval in milliseconds. */
  interval: number;
  /** Whether the timer restarts after each timeout (default true). */
  singleShot: boolean;
  start(): void;
  stop(): void;
  /** Fired each time the interval elapses. */
  readonly timeout: KWinSignal<void>;
}

declare const QTimer: { new (): KWinTimer };

/** A KWin scripting signal: connect/disconnect handlers fired on emission. */
interface KWinSignal<T> {
  connect(handler: (arg: T) => void): void;
  disconnect(handler: (arg: T) => void): void;
}

/** A QRect as exposed to KWin scripts. */
interface KWinRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface KWinOutput {
  readonly name: string;
  /** The output's placement and size in global compositor coordinates. */
  readonly geometry: KWinRect;
}

interface KWinVirtualDesktop {
  readonly id: string;
  readonly name: string;
}

interface KWinWindow {
  /** Stable per-window identity (QUuid as string). */
  readonly internalId: string;
  /** Resource class (the X11/Wayland app id), used for window-rule matching. */
  readonly resourceClass: string;
  /** Window caption (title), used for window-rule matching. */
  readonly caption: string;
  readonly output: KWinOutput;
  /** Desktops the window is on; empty means "all desktops". */
  readonly desktops: KWinVirtualDesktop[];
  /** Activity UUIDs the window is on; empty means "all activities". */
  readonly activities: string[];
  readonly normalWindow: boolean;
  readonly skipTaskbar: boolean;
  /** The window's frame geometry; assigning moves/resizes the window. */
  frameGeometry: KWinRect;
}

interface KWinWorkspace {
  readonly desktops: KWinVirtualDesktop[];
  readonly activities: string[];
  readonly screens: KWinOutput[];
  windowList(): KWinWindow[];
  /** The currently focused window; assigning it moves keyboard focus. */
  activeWindow: KWinWindow | null;
  /** Fired when the active window changes (null when focus is lost). */
  readonly windowActivated: KWinSignal<KWinWindow | null>;

  // Topology-change signals. Handlers ignore the payload and re-push the full
  // snapshot, so the arg types are kept loose where KWin emits multiple values.
  readonly windowAdded: KWinSignal<KWinWindow>;
  readonly windowRemoved: KWinSignal<KWinWindow>;
  readonly desktopsChanged: KWinSignal<void>;
  readonly currentDesktopChanged: KWinSignal<unknown>;
  readonly currentActivityChanged: KWinSignal<string>;
  readonly activitiesChanged: KWinSignal<string>;
  readonly virtualScreenGeometryChanged: KWinSignal<void>;
  readonly screensChanged: KWinSignal<void>;
}

declare const workspace: KWinWorkspace;
