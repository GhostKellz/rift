//! Unix-socket IPC server for the daemon.
//!
//! Owns the listening socket lifecycle and per-connection request handling, and
//! holds the shared [`State`] that every connection reconciles against.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rift_ipc::{
    Command, Event, PROTOCOL_VERSION, Reply, StatusReport, Topology, read_frame, write_frame,
};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::state::{MoveOutcome, State};

/// Shared daemon state behind the IPC server.
pub(crate) struct Daemon {
    start: Instant,
    state: State,
    /// Where the config is read from, for reloads and `riftctl config`.
    config_path: PathBuf,
    /// Whether a config file was actually present at the last (re)load. False
    /// means built-in defaults are in effect.
    config_loaded: bool,
}

impl Daemon {
    /// Assemble a daemon around an already-loaded [`State`]. Shared by the socket
    /// server, the D-Bus transport, and tests so they reconcile identical state.
    pub(crate) fn new(state: State, config_path: PathBuf, config_loaded: bool) -> Self {
        Daemon {
            start: Instant::now(),
            state,
            config_path,
            config_loaded,
        }
    }

    /// Re-read the config from disk, validate, and apply it.
    ///
    /// On any error the current config is retained and the diagnostic is
    /// returned, so a bad edit can never strand the daemon on a partial config.
    fn reload(&mut self) -> Result<(), String> {
        let cfg = Config::load(&self.config_path).map_err(|e| format!("{e:#}"))?;
        self.config_loaded = self.config_path.exists();
        self.state.apply_config(&cfg);
        Ok(())
    }

    /// Snapshot the effective config for `riftctl config`/`reload`.
    fn config_report(&self) -> rift_ipc::ConfigReport {
        self.state
            .config_report(self.config_path.display().to_string(), self.config_loaded)
    }
}

impl Daemon {
    /// Map an incoming request (a [`Command`] or an [`Event`]) to a [`Reply`].
    ///
    /// Kept free of I/O so it can be unit-tested directly and shared verbatim
    /// between the socket and D-Bus transports.
    pub(crate) fn dispatch(&mut self, value: serde_json::Value) -> Reply {
        if let Ok(cmd) = serde_json::from_value::<Command>(value.clone()) {
            return match cmd {
                Command::Status => Reply::Status(StatusReport {
                    version: rift_ipc::VERSION.to_string(),
                    protocol: PROTOCOL_VERSION,
                    uptime_secs: self.start.elapsed().as_secs(),
                    cells: self.state.cell_count(),
                    windows: self.state.window_count(),
                }),
                Command::Reset => Reply::Reconciled(self.state.reset()),
                Command::Focus { direction } => Reply::Focus {
                    window: self.state.focus_neighbor(direction),
                },
                Command::Move { direction } => {
                    // A cross-output move needs the script to re-push topology so
                    // the daemon re-keys the window on its new output.
                    match self.state.move_window(direction) {
                        MoveOutcome::CrossedOutput => Reply::GeometryResync(self.state.geometry()),
                        MoveOutcome::Swapped | MoveOutcome::None => {
                            Reply::Geometry(self.state.geometry())
                        }
                    }
                }
                Command::Resize { direction } => {
                    self.state.resize(direction);
                    Reply::Geometry(self.state.geometry())
                }
                Command::SetLayout { layout } => {
                    self.state.set_layout(layout);
                    Reply::Geometry(self.state.geometry())
                }
                Command::MasterRatio { delta } => {
                    self.state.adjust_master_ratio(delta);
                    Reply::Geometry(self.state.geometry())
                }
                Command::MasterCount { delta } => {
                    self.state.adjust_master_count(delta);
                    Reply::Geometry(self.state.geometry())
                }
                Command::ToggleTiling => {
                    self.state.toggle_tiling();
                    Reply::Geometry(self.state.geometry())
                }
                Command::ToggleFloat { window } => {
                    self.state.toggle_float(window);
                    Reply::Geometry(self.state.geometry())
                }
                Command::GetConfig => Reply::Config(self.config_report()),
                Command::GetKeybindings => Reply::Keybindings {
                    bindings: self.state.keybindings(),
                },
                Command::Reload => match self.reload() {
                    Ok(()) => {
                        info!(path = %self.config_path.display(), "config reloaded");
                        Reply::Config(self.config_report())
                    }
                    Err(message) => {
                        warn!(%message, "config reload rejected");
                        Reply::Error { message }
                    }
                },
            };
        }
        if let Ok(event) = serde_json::from_value::<Event>(value.clone()) {
            return match event {
                Event::Hello { .. } => Reply::Ack,
                Event::Topology(topo) => {
                    self.reconcile(&topo);
                    Reply::Geometry(self.state.geometry())
                }
                Event::Focus { window } => {
                    self.state.set_focus(window);
                    Reply::Ack
                }
            };
        }
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<missing>")
            .to_string();
        Reply::Error {
            message: format!("unsupported message: {kind}"),
        }
    }

    fn reconcile(&mut self, topo: &Topology) -> rift_ipc::ReconcileReport {
        let report = self.state.reconcile(topo);
        debug!(cells = report.cells, windows = report.windows, "reconciled");
        report
    }
}

/// A bound IPC server ready to accept connections.
pub struct Server {
    listener: UnixListener,
    socket_path: PathBuf,
    config_path: PathBuf,
    daemon: Arc<Mutex<Daemon>>,
}

impl Server {
    /// Bind the socket at `socket_path`, creating the parent directory with
    /// owner-only permissions and clearing any stale socket left behind by a
    /// previous run. The config at `config_path` is loaded and applied; a
    /// missing file falls back to built-in defaults.
    pub fn bind(socket_path: PathBuf, config_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent)?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        clear_stale_socket(&socket_path)?;

        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
        info!(path = %socket_path.display(), "listening");

        let config = Config::load(&config_path)?;
        let config_loaded = config_path.exists();
        let mut state = State::default();
        state.apply_config(&config);
        info!(
            path = %config_path.display(),
            loaded = config_loaded,
            "config applied"
        );

        Ok(Self {
            listener,
            socket_path,
            config_path: config_path.clone(),
            daemon: Arc::new(Mutex::new(Daemon::new(state, config_path, config_loaded))),
        })
    }

    /// Accept connections until `shutdown` resolves, then remove the socket.
    pub async fn serve(self, shutdown: impl std::future::Future<Output = ()>) {
        // Keep the watcher alive for the duration of `serve`; dropping it stops
        // delivery. It reloads the shared daemon on any change to the config file.
        let _watcher = spawn_config_watcher(self.config_path.clone(), Arc::clone(&self.daemon));

        // Best-effort: also expose the daemon on the session bus for the in-KWin
        // script, which cannot open sockets. Keep the connection alive for the
        // duration of `serve`; if there is no session bus (headless/CI), log and
        // carry on so the socket path still works.
        let _dbus = match crate::dbus::serve(
            Arc::clone(&self.daemon),
            crate::dbus::SERVICE_NAME,
            crate::dbus::OBJECT_PATH,
        )
        .await
        {
            Ok(conn) => {
                info!(name = crate::dbus::SERVICE_NAME, "serving D-Bus");
                Some(conn)
            }
            Err(e) => {
                warn!(error = %e, "D-Bus unavailable; in-KWin transport disabled");
                None
            }
        };

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, _addr)) => {
                        let daemon = Arc::clone(&self.daemon);
                        tokio::spawn(async move {
                            if let Err(e) = handle_conn(stream, daemon).await {
                                debug!(error = %e, "connection closed with error");
                            }
                        });
                    }
                    Err(e) => warn!(error = %e, "accept failed"),
                },
                _ = &mut shutdown => {
                    info!("shutting down");
                    break;
                }
            }
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

/// Remove a leftover socket file if no daemon is listening on it.
fn clear_stale_socket(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => anyhow::bail!("another riftd appears to be running at {}", path.display()),
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
            warn!(path = %path.display(), "removing stale socket");
            fs::remove_file(path)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Watch the config file for changes and reload the daemon on each one.
///
/// The watch is placed on the file's *parent directory* (non-recursive) and
/// filtered to events touching the config file, which is robust against editors
/// that replace-on-save (the file's inode changes, so a direct file watch would
/// go stale). Returns the live watcher, which must be kept alive to receive
/// events. If the parent directory is absent we skip watching with a warning
/// rather than create directories the user did not.
fn spawn_config_watcher(
    config_path: PathBuf,
    daemon: Arc<Mutex<Daemon>>,
) -> Option<notify::RecommendedWatcher> {
    use notify::{Event, RecursiveMode, Watcher};

    let dir = config_path.parent().map(Path::to_path_buf)?;
    if !dir.is_dir() {
        warn!(dir = %dir.display(), "config directory absent; live reload disabled");
        return None;
    }

    let watched = config_path.clone();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };
        if !event.paths.contains(&watched) {
            return;
        }
        let mut guard = daemon.lock().expect("daemon mutex poisoned");
        match guard.reload() {
            Ok(()) => info!(path = %watched.display(), "config reloaded (watch)"),
            Err(message) => warn!(%message, "config reload rejected (watch)"),
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            warn!(error = %e, "failed to create config watcher; live reload disabled");
            return None;
        }
    };

    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        warn!(error = %e, dir = %dir.display(), "failed to watch config directory");
        return None;
    }
    info!(dir = %dir.display(), "watching config for changes");
    Some(watcher)
}

/// Serve a single connection: read request frames, dispatch, reply.
async fn handle_conn(
    mut stream: UnixStream,
    daemon: Arc<Mutex<Daemon>>,
) -> Result<(), rift_ipc::IpcError> {
    let (mut reader, mut writer) = stream.split();

    loop {
        let value: serde_json::Value = match read_frame(&mut reader).await {
            Ok(v) => v,
            Err(rift_ipc::IpcError::UnexpectedEof) => return Ok(()),
            Err(e) => return Err(e),
        };
        // The lock is held only for the synchronous dispatch, never across await.
        let reply = {
            let mut guard = daemon.lock().expect("daemon mutex poisoned");
            guard.dispatch(value)
        };
        write_frame(&mut writer, &reply).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn daemon() -> Daemon {
        Daemon {
            start: Instant::now(),
            state: State::default(),
            config_path: PathBuf::from("/nonexistent/riftrc"),
            config_loaded: false,
        }
    }

    #[test]
    fn status_returns_report() {
        let reply = daemon().dispatch(json!({ "type": "Status" }));
        match reply {
            Reply::Status(r) => {
                assert_eq!(r.protocol, PROTOCOL_VERSION);
                assert_eq!(r.cells, 0);
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn hello_is_acked() {
        let reply =
            daemon().dispatch(json!({ "type": "Hello", "kwin_version": "6.2", "protocol": 1 }));
        assert_eq!(reply, Reply::Ack);
    }

    #[test]
    fn topology_reconciles_and_status_counts_update() {
        let mut d = daemon();
        let topo = json!({
            "type": "Topology",
            "outputs": [{
                "id": "o1", "name": "o1",
                "rect": { "x": 0, "y": 0, "width": 1920, "height": 1080 }
            }],
            "desktops": [{ "id": "d1", "name": "d1" }],
            "activities": [{ "id": "a1", "name": "a1" }],
            "windows": [{ "id": "w1", "output": "o1", "desktop": "d1", "activity": "a1" }]
        });
        match d.dispatch(topo) {
            Reply::Geometry(set) => assert_eq!(set.windows.len(), 1),
            other => panic!("expected Geometry, got {other:?}"),
        }

        match d.dispatch(json!({ "type": "Status" })) {
            Reply::Status(r) => {
                assert_eq!(r.cells, 1);
                assert_eq!(r.windows, 1);
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn toggle_tiling_gates_geometry_over_dispatch() {
        let mut d = daemon();
        let topo = json!({
            "type": "Topology",
            "outputs": [{
                "id": "o1", "name": "o1",
                "rect": { "x": 0, "y": 0, "width": 1920, "height": 1080 }
            }],
            "desktops": [{ "id": "d1", "name": "d1" }],
            "activities": [{ "id": "a1", "name": "a1" }],
            "windows": [
                { "id": "w1", "output": "o1", "desktop": "d1", "activity": "a1" },
                { "id": "w2", "output": "o1", "desktop": "d1", "activity": "a1" }
            ]
        });
        assert!(matches!(d.dispatch(topo), Reply::Geometry(set) if set.windows.len() == 2));

        match d.dispatch(json!({ "type": "ToggleTiling" })) {
            Reply::Geometry(set) => assert!(set.windows.is_empty()),
            other => panic!("expected empty Geometry, got {other:?}"),
        }
        match d.dispatch(json!({ "type": "ToggleTiling" })) {
            Reply::Geometry(set) => assert_eq!(set.windows.len(), 2),
            other => panic!("expected Geometry, got {other:?}"),
        }
    }

    #[test]
    fn toggle_float_excludes_focused_over_dispatch() {
        let mut d = daemon();
        let topo = json!({
            "type": "Topology",
            "outputs": [{
                "id": "o1", "name": "o1",
                "rect": { "x": 0, "y": 0, "width": 1920, "height": 1080 }
            }],
            "desktops": [{ "id": "d1", "name": "d1" }],
            "activities": [{ "id": "a1", "name": "a1" }],
            "windows": [
                { "id": "w1", "output": "o1", "desktop": "d1", "activity": "a1" },
                { "id": "w2", "output": "o1", "desktop": "d1", "activity": "a1" }
            ]
        });
        d.dispatch(topo);
        d.dispatch(json!({ "type": "Focus", "window": "w1" }));

        match d.dispatch(json!({ "type": "ToggleFloat", "window": null })) {
            Reply::Geometry(set) => {
                assert_eq!(set.windows.len(), 1);
                assert_eq!(set.windows[0].id, "w2".into());
            }
            other => panic!("expected Geometry, got {other:?}"),
        }
    }

    #[test]
    fn cross_output_move_replies_with_resync() {
        let mut d = daemon();
        let topo = json!({
            "type": "Topology",
            "outputs": [
                { "id": "o1", "name": "o1",
                  "rect": { "x": 0, "y": 0, "width": 1920, "height": 1080 } },
                { "id": "o2", "name": "o2",
                  "rect": { "x": 1920, "y": 0, "width": 1920, "height": 1080 } }
            ],
            "desktops": [{ "id": "d1", "name": "d1" }],
            "activities": [{ "id": "a1", "name": "a1" }],
            "windows": [{ "id": "w1", "output": "o1", "desktop": "d1", "activity": "a1" }]
        });
        d.dispatch(topo);
        d.dispatch(json!({ "type": "Focus", "window": "w1" }));

        // w1 is alone on o1, so moving right relocates it onto o2 and the daemon
        // asks the script to resync topology.
        match d.dispatch(json!({ "type": "Move", "direction": "Right" })) {
            Reply::GeometryResync(set) => {
                let wg = set
                    .windows
                    .iter()
                    .find(|g| g.id == "w1".into())
                    .expect("moved window has geometry");
                assert!(wg.rect.x >= 1920, "placed on the right output");
            }
            other => panic!("expected GeometryResync, got {other:?}"),
        }
    }

    #[test]
    fn get_keybindings_returns_table() {
        match daemon().dispatch(json!({ "type": "GetKeybindings" })) {
            Reply::Keybindings { bindings } => {
                assert!(!bindings.is_empty());
                assert!(bindings.iter().any(|b| b.id == "rift_focus_left"));
            }
            other => panic!("expected Keybindings, got {other:?}"),
        }
    }

    #[test]
    fn unknown_is_error() {
        let reply = daemon().dispatch(json!({ "type": "Nope" }));
        assert!(matches!(reply, Reply::Error { .. }));
    }

    #[test]
    fn get_config_returns_defaults_for_missing_file() {
        match daemon().dispatch(json!({ "type": "GetConfig" })) {
            Reply::Config(r) => {
                assert!(!r.loaded);
                assert_eq!(r.gaps_inner, 8); // built-in default
                assert_eq!(r.master_count, 1);
            }
            other => panic!("expected Config, got {other:?}"),
        }
    }

    #[test]
    fn reload_reads_from_config_path() {
        use std::io::Write;

        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "[gaps]\ninner = 4\nouter = 4\n").unwrap();

        let mut d = daemon();
        d.config_path = f.path().to_path_buf();

        match d.dispatch(json!({ "type": "Reload" })) {
            Reply::Config(r) => {
                assert!(r.loaded);
                assert_eq!(r.gaps_inner, 4);
                assert_eq!(r.gaps_outer, 4);
            }
            other => panic!("expected Config, got {other:?}"),
        }
    }

    #[test]
    fn invalid_reload_errors_and_retains_prior_config() {
        use std::io::Write;

        // Start from a valid file so the daemon holds a known-good config.
        let mut good = tempfile::NamedTempFile::new().unwrap();
        write!(good, "[gaps]\ninner = 4\nouter = 4\n").unwrap();
        let mut d = daemon();
        d.config_path = good.path().to_path_buf();
        assert!(matches!(
            d.dispatch(json!({ "type": "Reload" })),
            Reply::Config(_)
        ));

        // Point at an invalid file and reload: it must be rejected wholesale.
        let mut bad = tempfile::NamedTempFile::new().unwrap();
        write!(bad, "[layout]\nmaster_ratio = 9.0\n").unwrap();
        d.config_path = bad.path().to_path_buf();
        assert!(matches!(
            d.dispatch(json!({ "type": "Reload" })),
            Reply::Error { .. }
        ));

        // The prior good config (inner=4) is still in effect.
        match d.dispatch(json!({ "type": "GetConfig" })) {
            Reply::Config(r) => assert_eq!(r.gaps_inner, 4),
            other => panic!("expected Config, got {other:?}"),
        }
    }
}
