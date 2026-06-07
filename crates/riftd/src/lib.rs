//! Rift daemon library: IPC server, layout engine, cell model, and
//! reconciliation. Exposed as a library so integration tests can drive the
//! server directly.

pub mod config;
pub mod dbus;
pub mod keys;
pub mod layout;
pub mod server;
pub mod state;
