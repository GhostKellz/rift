//! The daemon-owned default keybinding table.
//!
//! The daemon is the single source of truth for shortcuts: the KWin script asks
//! for this table at handshake and registers each entry generically. Keeping it
//! here (rather than in the script) means a key can be remapped via `riftrc`
//! `[keys]` without touching the script, and the glyph keys that QKeySequence
//! requires for punctuation live in exactly one place.
//!
//! Punctuation keys MUST be the literal glyphs (`-`, `=`, `,`, `.`): QKeySequence
//! portable text has no names like `Minus`/`Comma`, so those parse to an unknown
//! key and never bind. The `Shift` variants avoid colliding with KDE defaults
//! (desktop zoom on `Meta+-`/`Meta+=`, Show Desktop on `Meta+T`, etc.), which
//! KGlobalAccel silently drops.

use rift_ipc::{Command, Direction, Keybinding, LayoutKind};

/// The built-in keybindings, in registration order.
///
/// Ids are stable (`rift_<group>_<name>`) so user rebinds in System Settings,
/// which KGlobalAccel keys by id, survive script reloads and `[keys]` overrides.
pub fn defaults() -> Vec<Keybinding> {
    let mut binds = Vec::new();

    // vim-style focus movement.
    for (letter, dir) in directions() {
        binds.push(Keybinding {
            id: format!("rift_focus_{}", dir_name(dir)),
            description: format!("Rift: Focus {}", dir_label(dir)),
            key: format!("Meta+{}", letter.to_ascii_uppercase()),
            command: Command::Focus { direction: dir },
        });
    }
    // vim-style window movement (Shift).
    for (letter, dir) in directions() {
        binds.push(Keybinding {
            id: format!("rift_move_{}", dir_name(dir)),
            description: format!("Rift: Move window {}", dir_label(dir)),
            key: format!("Meta+Shift+{}", letter.to_ascii_uppercase()),
            command: Command::Move { direction: dir },
        });
    }
    // Directional resize (Ctrl): Left/Right adjust the master split; Up/Down are
    // reserved (no-op today) but bound so the table is complete and documented.
    for (letter, dir) in directions() {
        binds.push(Keybinding {
            id: format!("rift_resize_{}", dir_name(dir)),
            description: format!("Rift: Resize {}", dir_label(dir)),
            key: format!("Meta+Ctrl+{}", letter.to_ascii_uppercase()),
            command: Command::Resize { direction: dir },
        });
    }

    // Layout selection. Tile (Meta+T) and ThreeColumn (Meta+D) collide with KDE
    // defaults, so they take Shift variants.
    binds.push(layout_bind("t", LayoutKind::Tile, Some("Meta+Shift+T")));
    binds.push(layout_bind("m", LayoutKind::Monocle, None));
    binds.push(layout_bind("c", LayoutKind::Columns, None));
    binds.push(layout_bind("s", LayoutKind::Spiral, None));
    binds.push(layout_bind(
        "d",
        LayoutKind::ThreeColumn,
        Some("Meta+Shift+D"),
    ));
    binds.push(layout_bind("f", LayoutKind::Floating, None));

    binds.push(Keybinding {
        id: "rift_toggle_tiling".into(),
        description: "Rift: Toggle auto-tiling".into(),
        key: "Meta+Y".into(),
        command: Command::ToggleTiling,
    });
    binds.push(Keybinding {
        // Meta+G is KDE's Grid View; Meta+Shift+Space is free and matches the
        // i3/sway float-toggle convention.
        id: "rift_toggle_float".into(),
        description: "Rift: Toggle float (focused)".into(),
        key: "Meta+Shift+Space".into(),
        command: Command::ToggleFloat { window: None },
    });

    // Master-area tuning. Glyph keys (not Minus/Equal/Comma/Period) per the
    // QKeySequence note above.
    binds.push(Keybinding {
        id: "rift_master_ratio_dec".into(),
        description: "Rift: Shrink master area".into(),
        key: "Meta+Shift+-".into(),
        command: Command::MasterRatio { delta: -0.05 },
    });
    binds.push(Keybinding {
        id: "rift_master_ratio_inc".into(),
        description: "Rift: Grow master area".into(),
        key: "Meta+Shift+=".into(),
        command: Command::MasterRatio { delta: 0.05 },
    });
    binds.push(Keybinding {
        id: "rift_master_count_dec".into(),
        description: "Rift: Fewer master windows".into(),
        key: "Meta+Shift+,".into(),
        command: Command::MasterCount { delta: -1 },
    });
    binds.push(Keybinding {
        id: "rift_master_count_inc".into(),
        description: "Rift: More master windows".into(),
        key: "Meta+Shift+.".into(),
        command: Command::MasterCount { delta: 1 },
    });

    binds
}

/// Whether `id` names a built-in binding, used to reject `[keys]` typos.
pub fn is_known_id(id: &str) -> bool {
    defaults().iter().any(|b| b.id == id)
}

/// The four directions paired with their vim letter.
fn directions() -> [(char, Direction); 4] {
    [
        ('h', Direction::Left),
        ('j', Direction::Down),
        ('k', Direction::Up),
        ('l', Direction::Right),
    ]
}

fn dir_name(dir: Direction) -> &'static str {
    match dir {
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

fn dir_label(dir: Direction) -> &'static str {
    match dir {
        Direction::Left => "Left",
        Direction::Right => "Right",
        Direction::Up => "Up",
        Direction::Down => "Down",
    }
}

/// Build a layout-selection binding; `key` overrides the default `Meta+<letter>`.
fn layout_bind(letter: &str, layout: LayoutKind, key: Option<&str>) -> Keybinding {
    let name = format!("{layout:?}").to_lowercase();
    Keybinding {
        id: format!("rift_layout_{name}"),
        description: format!("Rift: {layout:?} layout"),
        key: key
            .map(str::to_string)
            .unwrap_or_else(|| format!("Meta+{}", letter.to_ascii_uppercase())),
        command: Command::SetLayout { layout },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default table covers every group the script used to hardcode.
    #[test]
    fn defaults_cover_expected_ids() {
        let ids: Vec<String> = defaults().into_iter().map(|b| b.id).collect();
        for id in [
            "rift_focus_left",
            "rift_move_right",
            "rift_resize_left",
            "rift_layout_tile",
            "rift_layout_threecolumn",
            "rift_toggle_tiling",
            "rift_toggle_float",
            "rift_master_ratio_dec",
            "rift_master_count_inc",
        ] {
            assert!(ids.iter().any(|i| i == id), "missing binding id {id}");
        }
    }

    /// Punctuation bindings use the literal glyphs QKeySequence needs, never the
    /// `Minus`/`Comma` enum names that parse to an unknown key.
    #[test]
    fn master_keys_use_glyphs_not_names() {
        let by_id = |id: &str| defaults().into_iter().find(|b| b.id == id).unwrap().key;
        assert_eq!(by_id("rift_master_ratio_dec"), "Meta+Shift+-");
        assert_eq!(by_id("rift_master_ratio_inc"), "Meta+Shift+=");
        assert_eq!(by_id("rift_master_count_dec"), "Meta+Shift+,");
        assert_eq!(by_id("rift_master_count_inc"), "Meta+Shift+.");
    }

    /// Collision-prone layouts take their Shift variants.
    #[test]
    fn colliding_layouts_use_shift() {
        let by_id = |id: &str| defaults().into_iter().find(|b| b.id == id).unwrap().key;
        assert_eq!(by_id("rift_layout_tile"), "Meta+Shift+T");
        assert_eq!(by_id("rift_layout_threecolumn"), "Meta+Shift+D");
        assert_eq!(by_id("rift_layout_monocle"), "Meta+M");
    }

    #[test]
    fn is_known_id_rejects_unknown() {
        assert!(is_known_id("rift_focus_left"));
        assert!(!is_known_id("rift_focus_sideways"));
    }
}
