//! D-Bus transport for the in-KWin script.
//!
//! A KWin script cannot open sockets; its only IPC is the outbound, async
//! `callDBus`. We expose a single method, `Dispatch`, that carries the same JSON
//! body the Unix socket uses and routes it through [`Daemon::dispatch`]. The wire
//! protocol therefore has exactly one source of truth — the socket and the bus
//! reconcile against the same shared state.

use std::sync::{Arc, Mutex};

use rift_ipc::Reply;
use zbus::connection;

use crate::server::Daemon;
use crate::state::State;

/// Well-known bus name the daemon claims for the in-KWin transport.
pub const SERVICE_NAME: &str = "dev.ghostkellz.Rift";
/// Object path the [`RiftDbus`] interface is served at.
pub const OBJECT_PATH: &str = "/dev/ghostkellz/Rift";

/// The object exported on the session bus, wrapping the shared daemon so D-Bus
/// callers and socket clients reconcile against the same state.
pub struct RiftDbus {
    daemon: Arc<Mutex<Daemon>>,
}

#[zbus::interface(name = "dev.ghostkellz.Rift")]
impl RiftDbus {
    /// Dispatch one JSON request and return the JSON reply.
    ///
    /// This mirrors a single socket request/reply exactly: parse the body, route
    /// it through the daemon, serialize the reply. A malformed body yields a
    /// serialized `Reply::Error` rather than a D-Bus method error, so the script
    /// always sees a uniform reply envelope.
    fn dispatch(&self, json_in: &str) -> String {
        let reply = match serde_json::from_str::<serde_json::Value>(json_in) {
            Ok(value) => {
                let mut guard = self.daemon.lock().expect("daemon mutex poisoned");
                guard.dispatch(value)
            }
            Err(e) => Reply::Error {
                message: format!("invalid request JSON: {e}"),
            },
        };
        // `Reply` is always serializable; the fallback is purely defensive.
        serde_json::to_string(&reply).unwrap_or_else(|e| {
            format!("{{\"type\":\"Error\",\"message\":\"reply serialize failed: {e}\"}}")
        })
    }
}

/// Build a session-bus connection serving [`RiftDbus`] at `path` under
/// `service_name`.
///
/// The returned [`zbus::Connection`] must be kept alive for the service to stay
/// available; dropping it releases the name. Errors (e.g. no session bus)
/// propagate so the caller can degrade gracefully.
pub(crate) async fn serve(
    daemon: Arc<Mutex<Daemon>>,
    service_name: &str,
    path: &str,
) -> zbus::Result<zbus::Connection> {
    connection::Builder::session()?
        .name(service_name.to_string())?
        .serve_at(path.to_string(), RiftDbus { daemon })?
        .build()
        .await
}

/// Start a D-Bus service backed by a fresh default daemon.
///
/// Exposed for integration tests that exercise the real bus round-trip under a
/// unique name; production code uses [`serve`] with the shared daemon.
#[doc(hidden)]
pub async fn serve_default(service_name: &str, path: &str) -> zbus::Result<zbus::Connection> {
    let daemon = Arc::new(Mutex::new(Daemon::new(
        State::default(),
        std::path::PathBuf::from("/nonexistent/riftrc"),
        false,
    )));
    serve(daemon, service_name, path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn iface() -> RiftDbus {
        let daemon = Arc::new(Mutex::new(Daemon::new(
            State::default(),
            PathBuf::from("/nonexistent/riftrc"),
            false,
        )));
        RiftDbus { daemon }
    }

    #[test]
    fn dispatch_routes_status_to_reply() {
        let out = iface().dispatch("{\"type\":\"Status\"}");
        let reply: Reply = serde_json::from_str(&out).expect("reply is valid JSON");
        match reply {
            Reply::Status(r) => assert_eq!(r.protocol, rift_ipc::PROTOCOL_VERSION),
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_malformed_body_yields_error_reply() {
        let out = iface().dispatch("not json");
        let reply: Reply = serde_json::from_str(&out).expect("reply is valid JSON");
        assert!(matches!(reply, Reply::Error { .. }));
    }
}
