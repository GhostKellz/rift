//! `riftctl setup`: the idempotent per-user KDE integration step.
//!
//! A system-wide install drops the binaries, the KWin script, and the
//! `systemd --user` unit in place, but the per-user KDE state — enabling the
//! script, freeing `Meta+L`, clearing stale shortcut records, starting the
//! daemon — must happen in the user's session. This runs that, talking to the
//! live KGlobalAccel/KWin over D-Bus rather than editing config files the
//! running services own in memory (which they would revert; see tasks/lessons).
//!
//! Every step degrades gracefully: a missing helper (`qdbus6`, `gdbus`,
//! `kwriteconfig6`, `systemctl`) is warned about, not fatal, so the command is
//! safe to re-run and partial environments still get what they can.

use std::path::Path;
use std::process::Command;

use rift_ipc::{Command as IpcCommand, Reply, read_frame, write_frame};
use tokio::net::UnixStream;
use tokio::time::{Duration, sleep};

/// Ctrl+Alt+L as a single Qt key code: `Qt::CTRL|Qt::ALT|Qt::Key_L`
/// (`0x04000000 | 0x08000000 | 0x4C`). The conventional Linux lock chord, and
/// verified free of KDE defaults (`Meta+Escape`/`Meta+Shift+Escape` are taken).
const LOCK_KEY: i64 = 201326668;

/// The KGlobalAccel action tuple identifying KDE's Lock Session shortcut.
const LOCK_ACTION: &str = r#"["ksmserver", "Lock Session", "Session Management", "Lock Session"]"#;

/// Run the full setup. `no_service` skips the systemd enable/start (for users
/// who launch `riftd` another way).
pub async fn run(socket: &Path, no_service: bool) -> anyhow::Result<()> {
    if no_service {
        println!("==> Skipping systemd unit (per --no-service)");
    } else {
        enable_service();
    }

    let ids = fetch_binding_ids(socket).await;
    clear_kglobalaccel(&ids);
    relocate_lock_session();
    enable_kwin_script();
    reconfigure_kwin();

    println!("==> Done.");
    println!("    The rift KWin script is enabled and re-registers its shortcuts");
    println!("    with current defaults on load. KDE Lock Session was moved to");
    println!("    Ctrl+Alt+L so rift can own Meta+L for focus-right.");
    Ok(())
}

/// Enable and start the `systemd --user` unit. Idempotent: `enable --now` is a
/// no-op if it is already enabled and running.
fn enable_service() {
    println!("==> Enabling riftd.service (systemd --user)");
    if !run_cmd("systemctl", &["--user", "enable", "--now", "riftd.service"]) {
        eprintln!("warning: could not enable riftd.service; start `riftd` manually");
    }
}

/// Ask the running daemon for its keybinding ids so the KGlobalAccel clear
/// covers exactly what the script will register. Retries briefly because the
/// service may still be coming up. Returns an empty list (with a warning) if the
/// daemon never answers — the clear is then skipped, which is non-fatal.
async fn fetch_binding_ids(socket: &Path) -> Vec<String> {
    for attempt in 0..10 {
        if let Some(ids) = try_fetch_ids(socket).await {
            return ids;
        }
        if attempt == 0 {
            println!("==> Waiting for the daemon to answer (GetKeybindings)");
        }
        sleep(Duration::from_millis(250)).await;
    }
    eprintln!("warning: daemon did not answer GetKeybindings; skipping shortcut clear");
    Vec::new()
}

/// One attempt: connect, request the table, return the ids on success.
async fn try_fetch_ids(socket: &Path) -> Option<Vec<String>> {
    let mut stream = UnixStream::connect(socket).await.ok()?;
    write_frame(&mut stream, &IpcCommand::GetKeybindings)
        .await
        .ok()?;
    match read_frame(&mut stream).await.ok()? {
        Reply::Keybindings { bindings } => Some(bindings.into_iter().map(|b| b.id).collect()),
        _ => None,
    }
}

/// Remove each rift action's KGlobalAccel record so the script's next
/// `registerShortcut` is treated as brand-new and the current default applies.
/// KGlobalAccel otherwise keeps a stored (possibly empty) key over the script's
/// requested default, stranding remapped or once-cleared binds.
fn clear_kglobalaccel(ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    println!("==> Clearing stale rift shortcut records (KGlobalAccel)");
    let mut cleared = 0;
    for id in ids {
        // rift registers under the "kwin" component (KWin scripting shortcuts).
        if run_cmd_quiet(
            "qdbus6",
            &[
                "org.kde.kglobalaccel",
                "/kglobalaccel",
                "org.kde.KGlobalAccel.unregister",
                "kwin",
                id,
            ],
        ) {
            cleared += 1;
        }
    }
    if cleared == 0 {
        eprintln!("warning: qdbus6 unavailable; stale rift binds may keep empty shortcuts");
    } else {
        println!("    cleared {cleared} rift_* records under the kwin component");
    }
}

/// Move KDE's Lock Session off `Meta+L` (which rift wants for focus-right) to
/// `Ctrl+Alt+L`, via the live KGlobalAccel setter. A direct config edit does not
/// take — kglobalaccel owns the file in memory and reverts it — so this uses the
/// D-Bus method, which needs the key as four ints `[key,0,0,0]`, and verifies the
/// change read back.
fn relocate_lock_session() {
    println!("==> Freeing Meta+L (moving KDE Lock Session to Ctrl+Alt+L)");
    if which("gdbus").is_none() {
        eprintln!("warning: gdbus not found; set Lock Session to Ctrl+Alt+L manually");
        return;
    }

    backup_shortcuts_once();

    let keys_arg = format!("([{LOCK_KEY}, 0, 0, 0],)");
    let avail = capture(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.kde.kglobalaccel",
            "--object-path",
            "/kglobalaccel",
            "--method",
            "org.kde.KGlobalAccel.globalShortcutAvailable",
            &keys_arg,
            "kwin",
        ],
    );
    if !avail.contains("true") {
        println!("    note: Ctrl+Alt+L not free; leaving Lock Session unchanged");
        return;
    }

    let set_keys = format!("[([{LOCK_KEY}, 0, 0, 0],)]");
    run_cmd_quiet(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.kde.kglobalaccel",
            "--object-path",
            "/kglobalaccel",
            "--method",
            "org.kde.KGlobalAccel.setForeignShortcutKeys",
            LOCK_ACTION,
            &set_keys,
        ],
    );
    let readback = capture(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.kde.kglobalaccel",
            "--object-path",
            "/kglobalaccel",
            "--method",
            "org.kde.KGlobalAccel.shortcutKeys",
            LOCK_ACTION,
        ],
    );
    if readback.contains(&LOCK_KEY.to_string()) {
        println!("    Lock Session rebound to Ctrl+Alt+L (live)");
    } else {
        eprintln!("warning: Lock Session set did not read back; check System Settings");
    }
}

/// Back up `kglobalshortcutsrc` once before the live edit, mirroring the dev
/// script so a user can restore their prior chords.
fn backup_shortcuts_once() {
    let Some(cfg) = config_dir() else { return };
    let src = cfg.join("kglobalshortcutsrc");
    let bak = cfg.join("kglobalshortcutsrc.rift.bak");
    if src.exists() && !bak.exists() && std::fs::copy(&src, &bak).is_ok() {
        println!("    backed up {} -> {}", src.display(), bak.display());
    }
}

/// Enable the rift KWin script in `kwinrc`.
fn enable_kwin_script() {
    println!("==> Enabling the rift KWin script");
    if !run_cmd(
        "kwriteconfig6",
        &[
            "--file",
            "kwinrc",
            "--group",
            "Plugins",
            "--key",
            "rift-kwinEnabled",
            "true",
        ],
    ) {
        eprintln!("warning: kwriteconfig6 not found; enable 'rift-kwin' in System Settings");
    }
}

/// Ask KWin to reload its config so the script loads without a session restart.
fn reconfigure_kwin() {
    if !run_cmd_quiet("qdbus6", &["org.kde.KWin", "/KWin", "reconfigure"]) {
        eprintln!("warning: qdbus6 not found; reconfigure KWin from System Settings");
    }
}

/// `$XDG_CONFIG_HOME`, else `$HOME/.config`.
fn config_dir() -> Option<std::path::PathBuf> {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME")
        && !x.is_empty()
    {
        return Some(std::path::PathBuf::from(x));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".config"))
}

/// Run a command, returning whether it launched and exited zero. A missing
/// binary returns `false` (the caller decides whether that is a warning).
fn run_cmd(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Like [`run_cmd`] but silences the command's own stdout/stderr.
fn run_cmd_quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Capture a command's stdout as a string (empty on failure).
fn capture(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Resolve a program on `PATH`, like `command -v`.
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|cand| cand.is_file())
}
