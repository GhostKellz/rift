//! D-Bus transport round-trip over a real session bus.
//!
//! Skipped when no session bus is present (CI/headless) so the suite stays
//! hermetic. Uses a per-pid service name to avoid colliding with a real running
//! rift on a developer's bus.

use std::process;

use rift_ipc::Reply;
use riftd::dbus::{OBJECT_PATH, serve_default};

const INTERFACE: &str = "dev.ghostkellz.Rift";

/// Call `Dispatch` on the service and parse the JSON reply into a [`Reply`].
async fn dispatch(conn: &zbus::Connection, service: &str, body: &str) -> Reply {
    let msg = conn
        .call_method(
            Some(service),
            OBJECT_PATH,
            Some(INTERFACE),
            "Dispatch",
            &(body,),
        )
        .await
        .expect("Dispatch call");
    let json: String = msg.body().deserialize().expect("string reply");
    serde_json::from_str(&json).expect("reply is valid JSON")
}

#[tokio::test]
async fn dbus_dispatch_round_trips_status_and_topology() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("skipping dbus test: no session bus");
        return;
    }

    // Per-pid name so concurrent test runs / a live rift don't collide.
    let service = format!("dev.ghostkellz.RiftTest{}", process::id());
    let _server = serve_default(&service, OBJECT_PATH)
        .await
        .expect("serve on session bus");

    let client = zbus::Connection::session().await.expect("client session");

    // Status round-trips through the same dispatch the socket uses.
    match dispatch(&client, &service, "{\"type\":\"Status\"}").await {
        Reply::Status(r) => assert_eq!(r.protocol, rift_ipc::PROTOCOL_VERSION),
        other => panic!("expected Status, got {other:?}"),
    }

    // A topology event yields geometry for its two windows.
    let topo = r#"{"type":"Topology",
        "outputs":[{"id":"o1","name":"o1","rect":{"x":0,"y":0,"width":1920,"height":1080}}],
        "desktops":[{"id":"d1","name":"d1"}],
        "activities":[{"id":"a1","name":"a1"}],
        "windows":[
            {"id":"w1","output":"o1","desktop":"d1","activity":"a1"},
            {"id":"w2","output":"o1","desktop":"d1","activity":"a1"}
        ]}"#;
    match dispatch(&client, &service, topo).await {
        Reply::Geometry(set) => {
            assert_eq!(set.windows.len(), 2);
            let ids: Vec<_> = set.windows.iter().map(|w| w.id.to_string()).collect();
            assert!(ids.contains(&"w1".to_string()));
            assert!(ids.contains(&"w2".to_string()));
        }
        other => panic!("expected Geometry, got {other:?}"),
    }
}
