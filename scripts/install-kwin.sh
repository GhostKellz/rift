#!/bin/sh
# Build, install, and enable the rift-kwin script for the current user.
#
# Assembles a KWin KPackage at:
#   $XDG_DATA_HOME/kwin/scripts/rift/   (default ~/.local/share/...)
# then enables it via kwriteconfig6 and asks KWin to reconfigure.

set -eu

repo_root=$(cd "$(dirname "$0")/.." && pwd)
kwin_src="$repo_root/rift-kwin"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install_dir="$data_home/kwin/scripts/rift"

echo "==> Building rift-kwin"
npm --prefix "$kwin_src" install
npm --prefix "$kwin_src" run build

if [ ! -f "$kwin_src/contents/code/main.js" ]; then
    echo "error: build did not produce contents/code/main.js" >&2
    exit 1
fi

echo "==> Installing to $install_dir"
rm -rf "$install_dir"
mkdir -p "$install_dir/contents/code"
cp "$kwin_src/metadata.json" "$install_dir/metadata.json"
cp "$kwin_src/contents/code/main.js" "$install_dir/contents/code/main.js"

echo "==> Clearing stale rift shortcut records (KGlobalAccel D-Bus)"
# KGlobalAccel keeps an action's STORED active key over a script's requested
# default — even when the stored value is empty, which it reads as "the user
# cleared this." rift ids whose default changed (the Meta+Shift+T/D remaps) or
# that ever landed empty are then stuck and never pick up the script's new
# default. Removing each action's KGlobalAccel record makes the script's next
# registerShortcut be treated as brand-new, so the current default applies.
# rift registers under the "kwin" component (KWin scripting shortcuts).
if command -v qdbus6 >/dev/null 2>&1; then
    rift_actions="\
rift_focus_left rift_focus_down rift_focus_up rift_focus_right \
rift_move_left rift_move_down rift_move_up rift_move_right \
rift_layout_tile rift_layout_monocle rift_layout_columns rift_layout_spiral \
rift_layout_threecolumn rift_layout_floating \
rift_toggle_tiling rift_toggle_float \
rift_master_ratio_dec rift_master_ratio_inc \
rift_master_count_dec rift_master_count_inc"
    for action in $rift_actions; do
        qdbus6 org.kde.kglobalaccel /kglobalaccel \
            org.kde.KGlobalAccel.unregister kwin "$action" >/dev/null 2>&1 || true
    done
    echo "    cleared KGlobalAccel records for rift_* under the kwin component"
    echo "    (the script re-registers them with current defaults on next load)"
else
    echo "warning: qdbus6 not found; stale rift binds may keep empty shortcuts" >&2
fi

echo "==> Freeing Meta+L for rift (moving KDE Lock Session to Ctrl+Alt+L)"
# rift binds Meta+L to focus-right, which collides with KDE's "Lock Session"
# global. KGlobalAccel silently drops the loser, so reassign Lock first.
#
# Target is Ctrl+Alt+L (the conventional Linux lock chord, verified free): the
# planned Meta+Escape is taken by KDE System Monitor, and Meta+Shift+Escape too.
#
# A direct kglobalshortcutsrc edit does NOT take — the running kglobalaccel owns
# the file in memory and reverts it (tasks/lessons.md L4/L5). The reliable path
# is the live D-Bus setter, which needs the QKeySequence as FOUR ints
# [key,0,0,0]; a single int crashes kglobalaccel. qdbus6 can't marshal a(ai),
# so use gdbus. Ctrl+Alt+L = 0x04000000|0x08000000|0x4C = 201326668.
lock_action='["ksmserver", "Lock Session", "Session Management", "Lock Session"]'
lock_key=201326668
if command -v gdbus >/dev/null 2>&1; then
    # Back up the user's shortcut config once before touching it.
    ksrc="${XDG_CONFIG_HOME:-$HOME/.config}/kglobalshortcutsrc"
    if [ -f "$ksrc" ] && [ ! -f "$ksrc.rift.bak" ]; then
        cp "$ksrc" "$ksrc.rift.bak"
        echo "    backed up $ksrc -> $ksrc.rift.bak"
    fi
    avail=$(gdbus call --session --dest org.kde.kglobalaccel \
        --object-path /kglobalaccel \
        --method org.kde.KGlobalAccel.globalShortcutAvailable \
        "([$lock_key, 0, 0, 0],)" "kwin" 2>/dev/null || true)
    case "$avail" in
        *true*)
            gdbus call --session --dest org.kde.kglobalaccel \
                --object-path /kglobalaccel \
                --method org.kde.KGlobalAccel.setForeignShortcutKeys \
                "$lock_action" "[([$lock_key, 0, 0, 0],)]" >/dev/null 2>&1 || true
            readback=$(gdbus call --session --dest org.kde.kglobalaccel \
                --object-path /kglobalaccel \
                --method org.kde.KGlobalAccel.shortcutKeys \
                "$lock_action" 2>/dev/null || true)
            case "$readback" in
                *$lock_key*) echo "    Lock Session rebound to Ctrl+Alt+L (live)";;
                *) echo "warning: Lock Session set did not read back; check System Settings" >&2;;
            esac
            ;;
        *)
            echo "    note: Ctrl+Alt+L not free; leaving Lock Session unchanged"
            echo "          (Meta+L may still collide with rift focus-right)"
            ;;
    esac
else
    echo "warning: gdbus not found; can't move Lock Session off Meta+L" >&2
    echo "         set Lock Session to Ctrl+Alt+L manually in System Settings" >&2
fi

echo "==> Enabling KWin script"
if command -v kwriteconfig6 >/dev/null 2>&1; then
    kwriteconfig6 --file kwinrc --group Plugins --key rift-kwinEnabled true
else
    echo "warning: kwriteconfig6 not found; enable 'rift-kwin' manually" >&2
fi

if command -v qdbus6 >/dev/null 2>&1; then
    qdbus6 org.kde.KWin /KWin reconfigure || true
else
    echo "warning: qdbus6 not found; reconfigure KWin manually" >&2
fi

echo "==> Done. Start the daemon with: riftd"
echo "    rift's own shortcuts re-register with current defaults when the script"
echo "    loads (cleared above via KGlobalAccel D-Bus)."
echo "    KDE Lock Session was moved to Ctrl+Alt+L (live, via KGlobalAccel D-Bus)"
echo "    so rift can own Meta+L for focus-right."
