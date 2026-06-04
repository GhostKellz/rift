//! End-to-end IPC tests: drive the real socket server over a temp socket.

use std::path::PathBuf;
use std::time::Duration;

use rift_ipc::{
    Activity, Command, Desktop, Event, Output, PROTOCOL_VERSION, ReconcileReport, Rect, Reply,
    Topology, Window, read_frame, write_frame,
};
use riftd::server::Server;
use tokio::net::UnixStream;

/// Bind a server on a temp socket and serve until the returned guard is dropped.
async fn spawn_server() -> (PathBuf, tempfile::TempDir, tokio::sync::oneshot::Sender<()>) {
    spawn_server_with_config(None).await
}

/// As [`spawn_server`], but seed a `riftrc` in the temp dir with `config`.
///
/// Returns the config path alongside the socket so reload tests can rewrite it.
async fn spawn_server_with_config(
    config: Option<&str>,
) -> (PathBuf, tempfile::TempDir, tokio::sync::oneshot::Sender<()>) {
    let (socket, config_path, dir, tx) = spawn_server_full(config).await;
    let _ = config_path;
    (socket, dir, tx)
}

/// Full variant exposing the config path for tests that rewrite and reload it.
async fn spawn_server_full(
    config: Option<&str>,
) -> (
    PathBuf,
    PathBuf,
    tempfile::TempDir,
    tokio::sync::oneshot::Sender<()>,
) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("rift.sock");
    let config_path = dir.path().join("riftrc");
    if let Some(body) = config {
        std::fs::write(&config_path, body).unwrap();
    }
    let server = Server::bind(socket.clone(), config_path.clone()).expect("bind");

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        server
            .serve(async {
                let _ = rx.await;
            })
            .await;
    });

    // Wait for the socket to become connectable.
    for _ in 0..100 {
        if UnixStream::connect(&socket).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    (socket, config_path, dir, tx)
}

/// Rewrite a config atomically (temp file + rename), the way real editors save.
///
/// The daemon's watcher watches the parent directory precisely so it never
/// observes a transient empty file; a plain in-place `write` truncates first and
/// can race the watcher into applying defaults before the new bytes land.
fn atomic_write(path: &std::path::Path, content: &str) {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

#[tokio::test]
async fn status_round_trips() {
    let (socket, _dir, _stop) = spawn_server().await;

    let mut stream = UnixStream::connect(&socket).await.unwrap();
    write_frame(&mut stream, &Command::Status).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();

    match reply {
        Reply::Status(r) => {
            assert_eq!(r.protocol, PROTOCOL_VERSION);
            assert_eq!(r.version, rift_ipc::VERSION);
        }
        other => panic!("expected Status, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_is_acked() {
    let (socket, _dir, _stop) = spawn_server().await;

    let mut stream = UnixStream::connect(&socket).await.unwrap();
    let hello = Event::Hello {
        kwin_version: "6.2.0".into(),
        protocol: PROTOCOL_VERSION,
    };
    write_frame(&mut stream, &hello).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    assert_eq!(reply, Reply::Ack);
}

#[tokio::test]
async fn topology_then_reset_round_trip() {
    let (socket, _dir, _stop) = spawn_server().await;
    let mut stream = UnixStream::connect(&socket).await.unwrap();

    let topo = Topology {
        outputs: vec![Output {
            id: "o1".into(),
            name: "o1".into(),
            rect: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        }],
        desktops: vec![Desktop {
            id: "d1".into(),
            name: "d1".into(),
        }],
        activities: vec![Activity {
            id: "a1".into(),
            name: "a1".into(),
        }],
        windows: vec![
            Window {
                id: "w1".into(),
                output: "o1".into(),
                desktop: "d1".into(),
                activity: "a1".into(),
            },
            Window {
                id: "w2".into(),
                output: "o1".into(),
                desktop: "d1".into(),
                activity: "a1".into(),
            },
        ],
    };

    write_frame(&mut stream, &Event::Topology(topo))
        .await
        .unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    match reply {
        Reply::Geometry(set) => {
            // Both windows in one cell receive geometry under the default tile.
            let ids: Vec<String> = set.windows.iter().map(|g| g.id.to_string()).collect();
            assert_eq!(ids, vec!["w1".to_string(), "w2".to_string()]);
        }
        other => panic!("expected Geometry, got {other:?}"),
    }

    // Reset must rebuild an identical map from the retained topology.
    write_frame(&mut stream, &Command::Reset).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    assert_eq!(
        reply,
        Reply::Reconciled(ReconcileReport {
            cells: 1,
            windows: 2
        })
    );

    // Status should now reflect the reconciled counts.
    write_frame(&mut stream, &Command::Status).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    match reply {
        Reply::Status(r) => {
            assert_eq!(r.cells, 1);
            assert_eq!(r.windows, 2);
        }
        other => panic!("expected Status, got {other:?}"),
    }
}

#[tokio::test]
async fn config_loads_then_reload_reflects_rewrite() {
    let (socket, config_path, _dir, _stop) =
        spawn_server_full(Some("[gaps]\ninner = 20\nouter = 30\n")).await;
    let mut stream = UnixStream::connect(&socket).await.unwrap();

    // The seeded file is reflected by GetConfig.
    write_frame(&mut stream, &Command::GetConfig).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    match reply {
        Reply::Config(r) => {
            assert!(r.loaded);
            assert_eq!(r.gaps_inner, 20);
            assert_eq!(r.gaps_outer, 30);
        }
        other => panic!("expected Config, got {other:?}"),
    }

    // Rewrite the file and force a reload: new values take effect.
    atomic_write(&config_path, "[gaps]\ninner = 4\nouter = 4\n");
    write_frame(&mut stream, &Command::Reload).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    match reply {
        Reply::Config(r) => {
            assert_eq!(r.gaps_inner, 4);
            assert_eq!(r.gaps_outer, 4);
        }
        other => panic!("expected Config, got {other:?}"),
    }

    // An invalid rewrite is rejected and the prior config is retained.
    atomic_write(&config_path, "[layout]\nmaster_ratio = 9.0\n");
    write_frame(&mut stream, &Command::Reload).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    assert!(matches!(reply, Reply::Error { .. }));
    write_frame(&mut stream, &Command::GetConfig).await.unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    match reply {
        Reply::Config(r) => assert_eq!(r.gaps_inner, 4),
        other => panic!("expected Config, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_message_errors() {
    let (socket, _dir, _stop) = spawn_server().await;

    let mut stream = UnixStream::connect(&socket).await.unwrap();
    write_frame(&mut stream, &serde_json::json!({ "type": "Bogus" }))
        .await
        .unwrap();
    let reply: Reply = read_frame(&mut stream).await.unwrap();
    assert!(matches!(reply, Reply::Error { .. }));
}
