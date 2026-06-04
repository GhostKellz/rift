//! Shared IPC protocol types and length-prefixed JSON framing for Rift.
//!
//! Two logical channels share this protocol over a Unix domain socket:
//! - script <-> daemon: topology/window [`Event`]s, daemon replies with geometry/[`Reply`]
//! - CLI <-> daemon: control/query [`Command`]s, daemon replies with [`Reply`]
//!
//! The wire format is a 4-byte big-endian length prefix followed by a JSON body.
//! Bodies are bounded by [`MAX_FRAME`] and validated before deserialization.

use std::path::PathBuf;

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Wire protocol version. Bumped on any breaking change to message shapes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum accepted frame body size (1 MiB). Frames larger than this are
/// rejected before any allocation, bounding memory use on hostile input.
pub const MAX_FRAME: usize = 1 << 20;

/// Crate version, sourced from `Cargo.toml` (the single source of truth).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Declares a string-backed, transparently-(de)serialized identifier newtype.
///
/// These wrap the opaque identifiers KWin assigns to outputs, desktops,
/// activities, and windows. The newtypes keep the four kinds from being mixed
/// up when used as map keys, while serializing as bare JSON strings.
macro_rules! id_newtype {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
    };
}

id_newtype!(
    /// Identifier of a physical output (screen).
    OutputId
);
id_newtype!(
    /// Identifier of a virtual desktop.
    DesktopId
);
id_newtype!(
    /// Identifier of a Plasma activity.
    ActivityId
);
id_newtype!(
    /// Stable identifier of a managed window.
    WindowId
);

/// An axis-aligned rectangle in global compositor pixel coordinates.
///
/// Used both for an output's placement in the desktop and for the computed
/// geometry of a managed window. Signed to allow outputs positioned at
/// negative offsets in a multi-monitor layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A physical output present in the live topology.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Output {
    pub id: OutputId,
    pub name: String,
    /// The output's placement and size in global compositor coordinates.
    pub rect: Rect,
}

/// A virtual desktop present in the live topology.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Desktop {
    pub id: DesktopId,
    pub name: String,
}

/// A Plasma activity present in the live topology.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Activity {
    pub id: ActivityId,
    pub name: String,
}

/// A managed window and its placement within the topology.
///
/// A window is modeled as living on exactly one (output, desktop, activity)
/// tuple. Windows pinned to "all desktops"/"all activities" are resolved to a
/// concrete tuple by the script before forwarding (handled more richly later).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Window {
    pub id: WindowId,
    pub output: OutputId,
    pub desktop: DesktopId,
    pub activity: ActivityId,
}

/// A full snapshot of the live KWin topology.
///
/// The daemon treats this as the single source of truth on every reconcile;
/// its cell map is rebuilt from this snapshot, never restored from disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Topology {
    pub outputs: Vec<Output>,
    pub desktops: Vec<Desktop>,
    pub activities: Vec<Activity>,
    pub windows: Vec<Window>,
}

/// The tiling layout assigned to a cell.
///
/// Lives in the protocol because the layout can be switched over IPC
/// ([`Command::SetLayout`]) and is shared by the daemon's layout engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LayoutKind {
    #[default]
    Tile,
    Monocle,
    Columns,
    Spiral,
    ThreeColumn,
    Floating,
}

impl std::str::FromStr for LayoutKind {
    type Err = String;

    /// Parse a lowercase layout name, as used in the TOML config file.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tile" => Ok(Self::Tile),
            "monocle" => Ok(Self::Monocle),
            "columns" => Ok(Self::Columns),
            "spiral" => Ok(Self::Spiral),
            "threecolumn" => Ok(Self::ThreeColumn),
            "floating" => Ok(Self::Floating),
            other => Err(format!(
                "unknown layout {other:?} (expected one of: tile, monocle, \
                 columns, spiral, threecolumn, floating)"
            )),
        }
    }
}

/// A cardinal direction for focus and movement commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Events pushed from the KWin script to the daemon.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    /// Initial handshake sent by the script on connect.
    Hello { kwin_version: String, protocol: u32 },
    /// A full topology snapshot to reconcile against.
    Topology(Topology),
    /// The active window changed (or focus was lost: `window` is `None`).
    Focus { window: Option<WindowId> },
}

/// Commands sent from `riftctl` (or any client) to the daemon.
// No `Eq`: `MasterRatio { delta: f32 }` carries a float.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    /// Query daemon health and identity.
    Status,
    /// Force a full re-materialization of the cell map from the last topology.
    Reset,
    /// Move keyboard focus to the neighbor in `direction`.
    Focus { direction: Direction },
    /// Move the focused window toward `direction` within its cell.
    Move { direction: Direction },
    /// Switch the focused cell to `layout`.
    SetLayout { layout: LayoutKind },
    /// Adjust the master-area ratio by `delta` (clamped to a sane range).
    MasterRatio { delta: f32 },
    /// Adjust the master-window count by `delta` (never below one).
    MasterCount { delta: i32 },
    /// Re-read the config file from disk and apply it.
    Reload,
    /// Report the daemon's effective configuration.
    GetConfig,
}

/// Replies sent from the daemon back to a client.
// No `Eq`: `Config(ConfigReport)` carries a float (`master_ratio`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Reply {
    /// Generic success acknowledgement (e.g. for `Hello`).
    Ack,
    /// Response to [`Command::Status`].
    Status(StatusReport),
    /// Result of a reconcile (from [`Command::Reset`]).
    Reconciled(ReconcileReport),
    /// Computed window geometry for the script to apply (from
    /// [`Event::Topology`] or a control command that relayouts).
    Geometry(GeometrySet),
    /// The window the script should activate (from [`Command::Focus`]).
    /// `None` means no neighbor exists in the requested direction.
    Focus { window: Option<WindowId> },
    /// The daemon's effective configuration (from [`Command::GetConfig`] or a
    /// successful [`Command::Reload`]).
    Config(ConfigReport),
    /// A request could not be served.
    Error { message: String },
}

/// Daemon identity and health snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StatusReport {
    /// Daemon crate version.
    pub version: String,
    /// Wire protocol version the daemon speaks.
    pub protocol: u32,
    /// Seconds since the daemon started.
    pub uptime_secs: u64,
    /// Number of live cells after the last reconcile.
    pub cells: usize,
    /// Number of managed windows across all cells.
    pub windows: usize,
}

/// The daemon's effective configuration, flattened for display.
// No `Eq`: `master_ratio` is a float.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigReport {
    /// Default layout assigned to newly materialized cells.
    pub layout: LayoutKind,
    /// Fraction of the area given to the master region.
    pub master_ratio: f32,
    /// Number of windows in the master region.
    pub master_count: u32,
    /// Gap between adjacent tiles, in pixels.
    pub gaps_inner: i32,
    /// Gap between tiles and the output edge, in pixels.
    pub gaps_outer: i32,
    /// Whether layout state is tracked per virtual desktop (effect deferred).
    pub per_desktop: bool,
    /// Whether layout state is tracked per activity (effect deferred).
    pub per_activity: bool,
    /// Whether focus follows the mouse pointer (effect deferred).
    pub focus_follows_mouse: bool,
    /// Path the config was resolved from.
    pub source: String,
    /// Whether a config file was found and loaded (vs built-in defaults).
    pub loaded: bool,
}

/// Summary of a reconcile pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReconcileReport {
    /// Number of live cells after reconcile.
    pub cells: usize,
    /// Number of managed windows across all cells.
    pub windows: usize,
}

/// The computed target geometry for a single managed window.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowGeometry {
    pub id: WindowId,
    pub rect: Rect,
}

/// A batch of window geometries to apply in a single reconcile pass.
///
/// The script applies these as one update so a topology change yields one
/// coherent relayout rather than a stream of per-window moves.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GeometrySet {
    pub windows: Vec<WindowGeometry>,
}

/// Resolve the default daemon socket path.
///
/// Uses `$XDG_RUNTIME_DIR/rift/rift.sock`, falling back to
/// `/run/user/$UID/rift/rift.sock` when the variable is unset.
pub fn default_socket_path() -> PathBuf {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // SAFETY: getuid is always safe; it has no preconditions.
            let uid = unsafe { libc_getuid() };
            PathBuf::from(format!("/run/user/{uid}"))
        });
    runtime_dir.join("rift").join("rift.sock")
}

// Minimal getuid shim to avoid pulling in the `libc` crate for one call.
unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// Errors produced by the framing codec.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("frame too large: {0} bytes (max {MAX_FRAME})")]
    FrameTooLarge(usize),
    #[error("unexpected eof while reading frame")]
    UnexpectedEof,
}

/// Serialize a message into a length-prefixed JSON frame.
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, IpcError> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_FRAME {
        return Err(IpcError::FrameTooLarge(body.len()));
    }
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Read one length-prefixed JSON frame and deserialize it.
///
/// The length prefix is validated against [`MAX_FRAME`] before the body is
/// allocated or read.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, IpcError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(IpcError::UnexpectedEof);
        }
        Err(e) => return Err(IpcError::Io(e)),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(IpcError::FrameTooLarge(len));
    }
    let mut body = vec![0u8; len];
    match reader.read_exact(&mut body).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(IpcError::UnexpectedEof);
        }
        Err(e) => return Err(IpcError::Io(e)),
    }
    Ok(serde_json::from_slice(&body)?)
}

/// Encode and write one length-prefixed JSON frame, flushing the writer.
pub async fn write_frame<W, T>(writer: &mut W, msg: &T) -> Result<(), IpcError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let frame = encode(msg)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_command() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Command::Status).await.unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got: Command = read_frame(&mut cursor).await.unwrap();
        assert_eq!(got, Command::Status);
    }

    #[tokio::test]
    async fn round_trip_reply_status() {
        let report = StatusReport {
            version: "0.1.0".into(),
            protocol: PROTOCOL_VERSION,
            uptime_secs: 42,
            cells: 3,
            windows: 7,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &Reply::Status(report.clone()))
            .await
            .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got: Reply = read_frame(&mut cursor).await.unwrap();
        assert_eq!(got, Reply::Status(report));
    }

    #[tokio::test]
    async fn round_trip_event_hello() {
        let ev = Event::Hello {
            kwin_version: "6.2.0".into(),
            protocol: PROTOCOL_VERSION,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &ev).await.unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got: Event = read_frame(&mut cursor).await.unwrap();
        assert_eq!(got, ev);
    }

    #[tokio::test]
    async fn round_trip_event_topology() {
        let topo = Topology {
            outputs: vec![Output {
                id: "DP-1".into(),
                name: "DP-1".into(),
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            }],
            desktops: vec![Desktop {
                id: "d1".into(),
                name: "Desktop 1".into(),
            }],
            activities: vec![Activity {
                id: "a1".into(),
                name: "Default".into(),
            }],
            windows: vec![Window {
                id: "w1".into(),
                output: "DP-1".into(),
                desktop: "d1".into(),
                activity: "a1".into(),
            }],
        };
        let ev = Event::Topology(topo);
        let mut buf = Vec::new();
        write_frame(&mut buf, &ev).await.unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got: Event = read_frame(&mut cursor).await.unwrap();
        assert_eq!(got, ev);
    }

    #[tokio::test]
    async fn round_trip_control_command() {
        let cmd = Command::SetLayout {
            layout: LayoutKind::Spiral,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &cmd).await.unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got: Command = read_frame(&mut cursor).await.unwrap();
        assert_eq!(got, cmd);
    }

    #[test]
    fn control_command_serializes_with_type_tag() {
        let json = serde_json::to_string(&Command::Focus {
            direction: Direction::Left,
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"Focus","direction":"Left"}"#);
    }

    #[test]
    fn focus_reply_carries_optional_window() {
        let json = serde_json::to_string(&Reply::Focus {
            window: Some(WindowId::from("w7")),
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"Focus","window":"w7"}"#);
    }

    #[test]
    fn id_newtypes_serialize_as_bare_strings() {
        let json = serde_json::to_string(&WindowId::from("w42")).unwrap();
        assert_eq!(json, "\"w42\"");
    }

    #[test]
    fn layout_kind_parses_lowercase_names() {
        use std::str::FromStr;
        for (s, want) in [
            ("tile", LayoutKind::Tile),
            ("monocle", LayoutKind::Monocle),
            ("columns", LayoutKind::Columns),
            ("spiral", LayoutKind::Spiral),
            ("threecolumn", LayoutKind::ThreeColumn),
            ("floating", LayoutKind::Floating),
        ] {
            assert_eq!(LayoutKind::from_str(s).unwrap(), want);
        }
        assert!(LayoutKind::from_str("Tile").is_err());
        assert!(LayoutKind::from_str("bogus").is_err());
    }

    #[test]
    fn config_commands_serialize_with_type_tag() {
        assert_eq!(
            serde_json::to_string(&Command::Reload).unwrap(),
            r#"{"type":"Reload"}"#
        );
        assert_eq!(
            serde_json::to_string(&Command::GetConfig).unwrap(),
            r#"{"type":"GetConfig"}"#
        );
    }

    #[tokio::test]
    async fn round_trip_config_reply() {
        let report = ConfigReport {
            layout: LayoutKind::Columns,
            master_ratio: 0.55,
            master_count: 2,
            gaps_inner: 6,
            gaps_outer: 10,
            per_desktop: true,
            per_activity: false,
            focus_follows_mouse: true,
            source: "/tmp/riftrc".into(),
            loaded: true,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &Reply::Config(report.clone()))
            .await
            .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got: Reply = read_frame(&mut cursor).await.unwrap();
        assert_eq!(got, Reply::Config(report));
    }

    #[tokio::test]
    async fn oversize_prefix_rejected_before_alloc() {
        // A 4-byte prefix claiming a body far larger than MAX_FRAME, with no body.
        let mut frame = (MAX_FRAME as u32 + 1).to_be_bytes().to_vec();
        frame.extend_from_slice(b"");
        let mut cursor = std::io::Cursor::new(frame);
        let err = read_frame::<_, Command>(&mut cursor).await.unwrap_err();
        assert!(matches!(err, IpcError::FrameTooLarge(_)));
    }

    #[tokio::test]
    async fn truncated_body_is_unexpected_eof() {
        // Prefix says 10 bytes, but only 3 are present.
        let mut frame = 10u32.to_be_bytes().to_vec();
        frame.extend_from_slice(b"abc");
        let mut cursor = std::io::Cursor::new(frame);
        let err = read_frame::<_, Command>(&mut cursor).await.unwrap_err();
        assert!(matches!(err, IpcError::UnexpectedEof));
    }

    #[test]
    fn encode_rejects_oversize() {
        // serde_json of a huge string would exceed MAX_FRAME.
        let big = "x".repeat(MAX_FRAME + 1);
        let err = encode(&Reply::Error { message: big }).unwrap_err();
        assert!(matches!(err, IpcError::FrameTooLarge(_)));
    }
}
