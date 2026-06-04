//! Rift CLI: a thin client over the daemon's IPC socket.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use rift_ipc::{
    Command as IpcCommand, Direction, LayoutKind, Reply, default_socket_path, read_frame,
    write_frame,
};
use tokio::net::UnixStream;

#[derive(Parser)]
#[command(name = "riftctl", version, about = "Control and query the Rift daemon")]
struct Cli {
    /// Override the daemon socket path.
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show daemon health and identity.
    Status,
    /// Force a clean re-tile by re-materializing the cell map.
    Reset,
    /// Move keyboard focus to the neighbor in a direction.
    Focus {
        #[arg(value_enum)]
        direction: Dir,
    },
    /// Move the focused window toward a direction within its cell.
    Move {
        #[arg(value_enum)]
        direction: Dir,
    },
    /// Switch the focused cell to a layout.
    Layout {
        #[arg(value_enum)]
        kind: Layout,
    },
    /// Adjust the master-area ratio by a signed delta.
    MasterRatio { delta: f32 },
    /// Adjust the master window count by a signed delta.
    MasterCount { delta: i32 },
    /// Print the daemon's effective configuration.
    Config,
    /// Re-read the config from disk and apply it.
    Reload,
}

/// CLI-facing direction, mapped to [`Direction`] on the wire.
#[derive(Clone, Copy, ValueEnum)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl From<Dir> for Direction {
    fn from(d: Dir) -> Self {
        match d {
            Dir::Left => Direction::Left,
            Dir::Right => Direction::Right,
            Dir::Up => Direction::Up,
            Dir::Down => Direction::Down,
        }
    }
}

/// CLI-facing layout name, mapped to [`LayoutKind`] on the wire.
#[derive(Clone, Copy, ValueEnum)]
enum Layout {
    Tile,
    Monocle,
    Columns,
    Spiral,
    ThreeColumn,
    Floating,
}

impl From<Layout> for LayoutKind {
    fn from(l: Layout) -> Self {
        match l {
            Layout::Tile => LayoutKind::Tile,
            Layout::Monocle => LayoutKind::Monocle,
            Layout::Columns => LayoutKind::Columns,
            Layout::Spiral => LayoutKind::Spiral,
            Layout::ThreeColumn => LayoutKind::ThreeColumn,
            Layout::Floating => LayoutKind::Floating,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let socket = cli.socket.unwrap_or_else(default_socket_path);

    match cli.command {
        Cmd::Status => status(&socket).await,
        Cmd::Reset => reset(&socket).await,
        Cmd::Focus { direction } => focus(&socket, direction.into()).await,
        Cmd::Move { direction } => {
            relayout(
                &socket,
                IpcCommand::Move {
                    direction: direction.into(),
                },
            )
            .await
        }
        Cmd::Layout { kind } => {
            relayout(
                &socket,
                IpcCommand::SetLayout {
                    layout: kind.into(),
                },
            )
            .await
        }
        Cmd::MasterRatio { delta } => relayout(&socket, IpcCommand::MasterRatio { delta }).await,
        Cmd::MasterCount { delta } => relayout(&socket, IpcCommand::MasterCount { delta }).await,
        Cmd::Config => config(&socket, IpcCommand::GetConfig).await,
        Cmd::Reload => config(&socket, IpcCommand::Reload).await,
    }
}

async fn status(socket: &std::path::Path) -> anyhow::Result<()> {
    let reply = request(socket, &IpcCommand::Status)
        .await
        .with_context(|| format!("querying daemon at {}", socket.display()))?;

    match reply {
        Reply::Status(r) => {
            println!("riftd {} (protocol {})", r.version, r.protocol);
            println!("uptime: {}s", r.uptime_secs);
            println!("cells:  {}", r.cells);
            println!("windows: {}", r.windows);
        }
        Reply::Error { message } => anyhow::bail!("daemon error: {message}"),
        other => anyhow::bail!("unexpected reply to Status: {other:?}"),
    }
    Ok(())
}

async fn reset(socket: &std::path::Path) -> anyhow::Result<()> {
    let reply = request(socket, &IpcCommand::Reset)
        .await
        .with_context(|| format!("resetting daemon at {}", socket.display()))?;

    match reply {
        Reply::Reconciled(r) => {
            println!("reset: {} cells, {} windows", r.cells, r.windows);
        }
        Reply::Error { message } => anyhow::bail!("daemon error: {message}"),
        other => anyhow::bail!("unexpected reply to Reset: {other:?}"),
    }
    Ok(())
}

/// Move focus and report the window that received it.
async fn focus(socket: &std::path::Path, direction: Direction) -> anyhow::Result<()> {
    let reply = request(socket, &IpcCommand::Focus { direction })
        .await
        .with_context(|| format!("focusing {direction:?} via {}", socket.display()))?;

    match reply {
        Reply::Focus { window: Some(w) } => println!("focused: {w}"),
        Reply::Focus { window: None } => println!("no neighbor in that direction"),
        Reply::Error { message } => anyhow::bail!("daemon error: {message}"),
        other => anyhow::bail!("unexpected reply to Focus: {other:?}"),
    }
    Ok(())
}

/// Query (`GetConfig`) or reload (`Reload`) the daemon config, printing the
/// effective values it returns. A rejected reload surfaces the daemon's
/// diagnostic and exits non-zero, leaving the prior config in place.
async fn config(socket: &std::path::Path, cmd: IpcCommand) -> anyhow::Result<()> {
    let reply = request(socket, &cmd)
        .await
        .with_context(|| format!("sending {cmd:?} to {}", socket.display()))?;

    match reply {
        Reply::Config(c) => {
            let source = if c.loaded { "loaded" } else { "defaults" };
            println!("source:   {} ({source})", c.source);
            println!("layout:   {:?}", c.layout);
            println!(
                "master:   ratio {:.2}, count {}",
                c.master_ratio, c.master_count
            );
            println!("gaps:     inner {}, outer {}", c.gaps_inner, c.gaps_outer);
            println!(
                "behavior: per_desktop {}, per_activity {}, focus_follows_mouse {}",
                c.per_desktop, c.per_activity, c.focus_follows_mouse
            );
        }
        Reply::Error { message } => anyhow::bail!("daemon error: {message}"),
        other => anyhow::bail!("unexpected reply to {cmd:?}: {other:?}"),
    }
    Ok(())
}

/// Send a command that triggers a relayout and report the resulting window count.
async fn relayout(socket: &std::path::Path, cmd: IpcCommand) -> anyhow::Result<()> {
    let reply = request(socket, &cmd)
        .await
        .with_context(|| format!("sending {cmd:?} to {}", socket.display()))?;

    match reply {
        Reply::Geometry(set) => println!("relaid out {} windows", set.windows.len()),
        Reply::Error { message } => anyhow::bail!("daemon error: {message}"),
        other => anyhow::bail!("unexpected reply to {cmd:?}: {other:?}"),
    }
    Ok(())
}

/// Send one command and read one reply.
async fn request(socket: &std::path::Path, cmd: &IpcCommand) -> anyhow::Result<Reply> {
    let mut stream = UnixStream::connect(socket)
        .await
        .with_context(|| "is riftd running?")?;
    write_frame(&mut stream, cmd).await?;
    let reply = read_frame(&mut stream).await?;
    Ok(reply)
}
