//! Cell model and reconciliation.
//!
//! A *cell* is the layout state for one `(output, desktop, activity)` tuple.
//! Cells are **never the source of truth**: on every reconcile the cell map is
//! rebuilt from the live [`Topology`], so an orphaned cell — one whose output,
//! desktop, or activity has vanished — cannot survive. This is the failure mode
//! the project exists to eliminate.

use std::collections::{HashMap, HashSet};

use rift_ipc::{
    DesktopId, Direction, GeometrySet, Keybinding, LayoutKind, OutputId, ReconcileReport, Rect,
    Topology, WindowGeometry, WindowId,
};

use crate::config::{BehaviorConfig, Config, WindowRule};
use crate::keys;
use crate::layout::{self, LayoutParams};

/// Step applied to the master ratio by a directional [`State::resize`].
const RESIZE_STEP: f32 = 0.05;

/// Sentinel id used to collapse a cell dimension (desktop or activity) when its
/// `per_*` flag is off, so every value along that dimension shares one cell. The
/// leading NUL keeps it from colliding with any real compositor id.
const COLLAPSED: &str = "\0rift-all";

/// Identifies a cell by its `(output, desktop, activity)` tuple.
///
/// When `per_desktop`/`per_activity` is off the corresponding field holds
/// [`COLLAPSED`] instead of the live id, merging that dimension into one cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub output: OutputId,
    pub desktop: DesktopId,
    pub activity: rift_ipc::ActivityId,
}

/// The outcome of [`State::move_window`].
///
/// Distinguishes an in-cell swap from a relocation across outputs: the latter
/// needs the script to re-push topology so the daemon can re-key the window on
/// its new output (see [`rift_ipc::Reply::GeometryResync`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    /// Nothing moved (no focus, or no neighbor and no adjacent output).
    None,
    /// Swapped with a neighbor inside the same cell.
    Swapped,
    /// Relocated to an adjacent output's cell; the script must resync topology.
    CrossedOutput,
}

/// Per-cell layout state: the ordered window list and the active layout.
#[derive(Debug, Clone)]
pub struct Cell {
    /// Windows in this cell, in a stable order used by layouts.
    pub windows: Vec<WindowId>,
    /// Layout assigned to this cell.
    pub layout: LayoutKind,
}

/// The daemon's reconciled view of the world.
#[derive(Debug)]
pub struct State {
    cells: HashMap<CellKey, Cell>,
    default_layout: LayoutKind,
    /// Layout tunables (ratios, counts, gaps) applied to every cell.
    params: LayoutParams,
    /// The active window as last reported by the script, validated against the
    /// live topology on every reconcile so it can never point at a dead window.
    focused: Option<WindowId>,
    /// Session behavior flags from config. `per_desktop`/`per_activity` gate how
    /// [`State::reconcile`] keys cells (collapsing a dimension when off);
    /// `focus_follows_mouse` is surfaced but not yet acted on.
    behavior: BehaviorConfig,
    /// Whether global auto-tiling is enabled. When false, [`State::geometry`]
    /// emits nothing so every window floats where KWin last left it.
    tiling_enabled: bool,
    /// Windows the user has explicitly floated; the layout engine skips them.
    /// Pruned to the live topology on every reconcile.
    floating_windows: HashSet<WindowId>,
    /// Window rules from config, matched on class/title each reconcile.
    rules: Vec<WindowRule>,
    /// Windows excluded from tiling by a matching `float` rule. Recomputed from
    /// the topology on every reconcile (rules match live class/title, not ids).
    ruled_float: HashSet<WindowId>,
    /// `[keys]` overrides (binding id -> key sequence) applied over the default
    /// table when the script asks for [`State::keybindings`].
    key_overrides: HashMap<String, String>,
    /// The most recent topology, retained so [`State::reset`] can rebuild.
    last_topology: Topology,
}

impl Default for State {
    fn default() -> Self {
        // `tiling_enabled` must default to `true`; the derived bool default is
        // `false`, which would silently ship with tiling off.
        Self {
            cells: HashMap::new(),
            default_layout: LayoutKind::default(),
            params: LayoutParams::default(),
            focused: None,
            behavior: BehaviorConfig::default(),
            tiling_enabled: true,
            floating_windows: HashSet::new(),
            rules: Vec::new(),
            ruled_float: HashSet::new(),
            key_overrides: HashMap::new(),
            last_topology: Topology::default(),
        }
    }
}

impl State {
    /// Number of live cells.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Total managed windows across all cells.
    pub fn window_count(&self) -> usize {
        self.cells.values().map(|c| c.windows.len()).sum()
    }

    /// Read-only access to the cell map (for tests and, later, layout output).
    pub fn cells(&self) -> &HashMap<CellKey, Cell> {
        &self.cells
    }

    fn report(&self) -> ReconcileReport {
        ReconcileReport {
            cells: self.cell_count(),
            windows: self.window_count(),
        }
    }

    /// Rebuild the cell map from a fresh topology snapshot.
    ///
    /// 1. Drop windows that reference a nonexistent output/desktop/activity.
    /// 2. Drop cells whose tuple is no longer present.
    /// 3. Prune window references that have left a cell.
    /// 4. Materialize cells for newly present tuples with the default layout.
    /// 5. Append newly present windows to their cell in topology order.
    pub fn reconcile(&mut self, topology: &Topology) -> ReconcileReport {
        // Guard against a transient empty topology. On resume-from-suspend or
        // monitor hotplug, KWin can momentarily report zero outputs while it
        // reconfigures; wiping the cell map on that blip would destroy the layout.
        // If we had outputs last tick and now see none, hold the prior state and
        // wait for the next non-empty topology to relayout. This is bounded to
        // exactly that case so a first-ever empty topology still no-ops normally.
        if topology.outputs.is_empty() && !self.last_topology.outputs.is_empty() {
            return self.report();
        }

        let outputs: HashSet<&OutputId> = topology.outputs.iter().map(|o| &o.id).collect();
        let desktops: HashSet<&DesktopId> = topology.desktops.iter().map(|d| &d.id).collect();
        let activities: HashSet<&rift_ipc::ActivityId> =
            topology.activities.iter().map(|a| &a.id).collect();

        // (1) Map present tuples -> ordered windows, skipping orphan references.
        //
        // Orphan detection always checks the *raw* topology ids so a window on a
        // vanished desktop/activity is still dropped; only the cell *key* is
        // collapsed when a `per_*` flag is off, merging that dimension's windows
        // into a single shared cell.
        let mut present: HashMap<CellKey, Vec<WindowId>> = HashMap::new();
        for w in &topology.windows {
            if !outputs.contains(&w.output)
                || !desktops.contains(&w.desktop)
                || !activities.contains(&w.activity)
            {
                continue;
            }
            let key = CellKey {
                output: w.output.clone(),
                desktop: if self.behavior.per_desktop {
                    w.desktop.clone()
                } else {
                    DesktopId::from(COLLAPSED)
                },
                activity: if self.behavior.per_activity {
                    w.activity.clone()
                } else {
                    rift_ipc::ActivityId::from(COLLAPSED)
                },
            };
            present.entry(key).or_default().push(w.id.clone());
        }

        // (2) Drop dead cells.
        self.cells.retain(|key, _| present.contains_key(key));

        // (3-5) Reconcile each present tuple.
        for (key, topo_windows) in &present {
            let cell = self.cells.entry(key.clone()).or_insert_with(|| Cell {
                windows: Vec::new(),
                layout: self.default_layout,
            });

            // (3) Prune refs no longer in this cell.
            let live: HashSet<&WindowId> = topo_windows.iter().collect();
            cell.windows.retain(|w| live.contains(w));

            // (5) Append windows new to this cell, in topology order.
            let existing: HashSet<WindowId> = cell.windows.iter().cloned().collect();
            for w in topo_windows {
                if !existing.contains(w) {
                    cell.windows.push(w.clone());
                }
            }
        }

        // Focus can never survive its window: drop it if the window is gone.
        if let Some(f) = &self.focused
            && !self.cells.values().any(|c| c.windows.contains(f))
        {
            self.focused = None;
        }

        // Floating marks cannot outlive their windows either.
        let live_windows: HashSet<&WindowId> = topology.windows.iter().map(|w| &w.id).collect();
        self.floating_windows.retain(|w| live_windows.contains(w));

        // Recompute rule-driven floats from live class/title. Rules match window
        // metadata, not ids, so this is rebuilt from scratch each reconcile.
        self.ruled_float = if self.rules.is_empty() {
            HashSet::new()
        } else {
            topology
                .windows
                .iter()
                .filter(|w| {
                    self.rules
                        .iter()
                        .any(|r| r.float && r.matches(w.class.as_deref(), w.title.as_deref()))
                })
                .map(|w| w.id.clone())
                .collect()
        };

        self.last_topology = topology.clone();
        self.report()
    }

    /// Force a full re-materialization: discard all cells and rebuild from the
    /// last known topology. This is the recovery path behind `riftctl reset`.
    pub fn reset(&mut self) -> ReconcileReport {
        self.cells.clear();
        let topology = std::mem::take(&mut self.last_topology);
        self.reconcile(&topology)
    }

    /// Compute the target geometry for every managed window.
    ///
    /// Each cell is laid out within its output's rectangle; the results are
    /// emitted in topology order so the output is deterministic. Windows whose
    /// cell imposes no geometry (e.g. floating) are simply absent.
    pub fn geometry(&self) -> GeometrySet {
        // Auto-tiling off: emit nothing so every window floats untouched.
        if !self.tiling_enabled {
            return GeometrySet::default();
        }

        let output_rects: HashMap<&OutputId, Rect> = self
            .last_topology
            .outputs
            .iter()
            .map(|o| (&o.id, o.rect))
            .collect();

        let mut placed: HashMap<WindowId, Rect> = HashMap::new();
        for (key, cell) in &self.cells {
            let Some(&area) = output_rects.get(&key.output) else {
                continue;
            };
            // Floated windows (user-toggled or rule-matched) are excluded from
            // the layout; the rest re-tile.
            let tiled: Vec<WindowId> = cell
                .windows
                .iter()
                .filter(|w| !self.floating_windows.contains(*w) && !self.ruled_float.contains(*w))
                .cloned()
                .collect();
            for wg in layout::arrange(cell.layout, &tiled, area, &self.params) {
                placed.insert(wg.id, wg.rect);
            }
        }

        let windows = self
            .last_topology
            .windows
            .iter()
            .filter_map(|w| {
                placed.get(&w.id).map(|&rect| WindowGeometry {
                    id: w.id.clone(),
                    rect,
                })
            })
            .collect();
        GeometrySet { windows }
    }

    /// The active window, if one is known and still live.
    pub fn focused(&self) -> Option<&WindowId> {
        self.focused.as_ref()
    }

    /// Record the active window as reported by the script.
    pub fn set_focus(&mut self, window: Option<WindowId>) {
        self.focused = window;
    }

    /// The cell currently holding `window`, if any.
    fn cell_of(&self, window: &WindowId) -> Option<CellKey> {
        self.cells
            .iter()
            .find(|(_, c)| c.windows.contains(window))
            .map(|(k, _)| k.clone())
    }

    /// Resolve the window to focus when moving `direction` from the active one.
    ///
    /// Neighbors are found spatially from the computed global geometry, so this
    /// works across layouts and across outputs. Returns `None` when no window
    /// lies in that direction (or nothing is focused).
    pub fn focus_neighbor(&self, direction: Direction) -> Option<WindowId> {
        let focused = self.focused.as_ref()?;
        let geoms = self.geometry().windows;
        layout::neighbor(&geoms, focused, direction)
    }

    /// Move the focused window in `direction`.
    ///
    /// Within a cell this swaps the window with its directional neighbor. When
    /// the window is already at the cell's edge and a spatially adjacent output
    /// lies that way, it is relocated onto that output's cell instead — the
    /// [`MoveOutcome::CrossedOutput`] case the caller turns into a topology
    /// resync. Returns what kind of move (if any) happened.
    pub fn move_window(&mut self, direction: Direction) -> MoveOutcome {
        let Some(focused) = self.focused.clone() else {
            return MoveOutcome::None;
        };
        let Some(key) = self.cell_of(&focused) else {
            return MoveOutcome::None;
        };
        // Restrict the neighbor search to this cell so a within-cell move stays
        // local; a missing neighbor means the window sits at the cell's edge.
        let cell_ids: HashSet<WindowId> = self.cells[&key].windows.iter().cloned().collect();
        let local: Vec<WindowGeometry> = self
            .geometry()
            .windows
            .into_iter()
            .filter(|g| cell_ids.contains(&g.id))
            .collect();

        if let Some(neighbor) = layout::neighbor(&local, &focused, direction) {
            let cell = self.cells.get_mut(&key).expect("focused cell exists");
            let (Some(i), Some(j)) = (
                cell.windows.iter().position(|w| *w == focused),
                cell.windows.iter().position(|w| *w == neighbor),
            ) else {
                return MoveOutcome::None;
            };
            cell.windows.swap(i, j);
            return MoveOutcome::Swapped;
        }

        // At the cell edge: relocate to the adjacent output that way, if any.
        let Some(target_output) = self.adjacent_output(&key.output, direction) else {
            return MoveOutcome::None;
        };
        self.cells
            .get_mut(&key)
            .expect("focused cell exists")
            .windows
            .retain(|w| *w != focused);
        let dest = CellKey {
            output: target_output,
            desktop: key.desktop.clone(),
            activity: key.activity.clone(),
        };
        self.cells
            .entry(dest)
            .or_insert_with(|| Cell {
                windows: Vec::new(),
                layout: self.default_layout,
            })
            .windows
            .push(focused);
        MoveOutcome::CrossedOutput
    }

    /// Find the output spatially adjacent to `output` in `direction`.
    ///
    /// Picks from the last topology's output rectangles by comparing centers:
    /// only outputs whose center lies that way are eligible, nearest along the
    /// axis wins with perpendicular drift penalized. Returns `None` when no
    /// output lies in that direction.
    fn adjacent_output(&self, output: &OutputId, direction: Direction) -> Option<OutputId> {
        let origin = self
            .last_topology
            .outputs
            .iter()
            .find(|o| &o.id == output)?
            .rect;
        let (fx, fy) = (origin.x + origin.width / 2, origin.y + origin.height / 2);

        let mut best: Option<(i64, &OutputId)> = None;
        for o in &self.last_topology.outputs {
            if &o.id == output {
                continue;
            }
            let (cx, cy) = (o.rect.x + o.rect.width / 2, o.rect.y + o.rect.height / 2);
            let (dx, dy) = (cx - fx, cy - fy);
            let in_dir = match direction {
                Direction::Left => dx < 0,
                Direction::Right => dx > 0,
                Direction::Up => dy < 0,
                Direction::Down => dy > 0,
            };
            if !in_dir {
                continue;
            }
            let (along, perp) = match direction {
                Direction::Left | Direction::Right => (dx.unsigned_abs(), dy.unsigned_abs()),
                Direction::Up | Direction::Down => (dy.unsigned_abs(), dx.unsigned_abs()),
            };
            let score = along as i64 + 2 * perp as i64;
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, &o.id));
            }
        }
        best.map(|(_, id)| id.clone())
    }

    /// Resize the focused window's split in `direction`.
    ///
    /// `Left`/`Right` shrink/grow the master area (reusing the master-ratio
    /// path, which clamps to a sane range). `Up`/`Down` are reserved — the
    /// vertical split is layout-specific and not yet adjustable — so they
    /// no-op. Returns whether anything changed.
    pub fn resize(&mut self, direction: Direction) -> bool {
        match direction {
            Direction::Left => {
                self.adjust_master_ratio(-RESIZE_STEP);
                true
            }
            Direction::Right => {
                self.adjust_master_ratio(RESIZE_STEP);
                true
            }
            Direction::Up | Direction::Down => false,
        }
    }

    /// The effective keybinding table: the built-in defaults with any `[keys]`
    /// overrides applied by id. Served to the script on `GetKeybindings`.
    pub fn keybindings(&self) -> Vec<Keybinding> {
        let mut table = keys::defaults();
        if !self.key_overrides.is_empty() {
            for kb in &mut table {
                if let Some(key) = self.key_overrides.get(&kb.id) {
                    kb.key = key.clone();
                }
            }
        }
        table
    }

    /// Switch the focused cell to `layout`. Returns whether a cell was changed.
    pub fn set_layout(&mut self, layout: LayoutKind) -> bool {
        let Some(focused) = self.focused.clone() else {
            return false;
        };
        let Some(key) = self.cell_of(&focused) else {
            return false;
        };
        self.cells
            .get_mut(&key)
            .expect("focused cell exists")
            .layout = layout;
        true
    }

    /// Flip global auto-tiling. Returns the new enabled state.
    pub fn toggle_tiling(&mut self) -> bool {
        self.tiling_enabled = !self.tiling_enabled;
        self.tiling_enabled
    }

    /// Toggle the floating state of `window` (or the focused window when
    /// `None`). A floated window is skipped by the layout engine. Returns the
    /// new floating state, or `None` when no window could be resolved.
    pub fn toggle_float(&mut self, window: Option<WindowId>) -> Option<bool> {
        let target = window.or_else(|| self.focused.clone())?;
        if self.floating_windows.remove(&target) {
            Some(false)
        } else {
            self.floating_windows.insert(target);
            Some(true)
        }
    }

    /// Adjust the master-area ratio, clamped to a usable range.
    pub fn adjust_master_ratio(&mut self, delta: f32) {
        self.params.master_ratio = (self.params.master_ratio + delta).clamp(0.05, 0.95);
    }

    /// Adjust the master-window count, never dropping below one.
    pub fn adjust_master_count(&mut self, delta: i32) {
        let next = self.params.master_count as i32 + delta;
        self.params.master_count = next.max(1) as usize;
    }

    /// Apply a validated config to the running state.
    ///
    /// `default` only affects *newly materialized* cells; ratio and gaps take
    /// effect at the next [`State::geometry`] (i.e. the next reconcile/relayout).
    /// `per_desktop`/`per_activity` re-key cells on the next reconcile.
    pub fn apply_config(&mut self, cfg: &Config) {
        self.default_layout = cfg.layout.default;
        self.params.master_ratio = cfg.layout.master_ratio;
        self.params.master_count = cfg.layout.master_count;
        self.params.gaps_inner = cfg.gaps.inner;
        self.params.gaps_outer = cfg.gaps.outer;
        self.tiling_enabled = cfg.layout.tiling_enabled;
        self.behavior = cfg.behavior.clone();
        self.rules = cfg.rules.clone();
        self.key_overrides = cfg.keys.clone();
    }

    /// Snapshot the effective config for `riftctl config`/`reload`.
    ///
    /// `source` is the config path the daemon resolved; `loaded` is whether a
    /// file was actually present (false means built-in defaults are in effect).
    pub fn config_report(&self, source: String, loaded: bool) -> rift_ipc::ConfigReport {
        rift_ipc::ConfigReport {
            layout: self.default_layout,
            master_ratio: self.params.master_ratio,
            master_count: self.params.master_count as u32,
            gaps_inner: self.params.gaps_inner,
            gaps_outer: self.params.gaps_outer,
            per_desktop: self.behavior.per_desktop,
            per_activity: self.behavior.per_activity,
            focus_follows_mouse: self.behavior.focus_follows_mouse,
            tiling_enabled: self.tiling_enabled,
            source,
            loaded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rift_ipc::{Activity, Desktop, Output, Rect, Window};

    fn output(id: &str) -> Output {
        Output {
            id: id.into(),
            name: id.into(),
            rect: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        }
    }
    /// An output placed at horizontal offset `x` (for multi-monitor tests).
    fn output_at(id: &str, x: i32) -> Output {
        Output {
            id: id.into(),
            name: id.into(),
            rect: Rect {
                x,
                y: 0,
                width: 1920,
                height: 1080,
            },
        }
    }
    fn desktop(id: &str) -> Desktop {
        Desktop {
            id: id.into(),
            name: id.into(),
        }
    }
    fn activity(id: &str) -> Activity {
        Activity {
            id: id.into(),
            name: id.into(),
        }
    }
    fn window(id: &str, output: &str, desktop: &str, activity: &str) -> Window {
        Window {
            id: id.into(),
            output: output.into(),
            desktop: desktop.into(),
            activity: activity.into(),
            class: None,
            title: None,
        }
    }

    fn window_classed(id: &str, output: &str, class: &str) -> Window {
        Window {
            id: id.into(),
            output: output.into(),
            desktop: "d1".into(),
            activity: "a1".into(),
            class: Some(class.into()),
            title: None,
        }
    }

    fn key(output: &str, desktop: &str, activity: &str) -> CellKey {
        CellKey {
            output: output.into(),
            desktop: desktop.into(),
            activity: activity.into(),
        }
    }

    /// A single window materializes exactly one cell holding it.
    #[test]
    fn materializes_cell_for_window() {
        let mut state = State::default();
        let topo = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        };
        let report = state.reconcile(&topo);
        assert_eq!(report.cells, 1);
        assert_eq!(report.windows, 1);
        assert_eq!(
            state.cells()[&key("o1", "d1", "a1")].windows,
            vec!["w1".into()]
        );
    }

    /// Window order is preserved across reconciles; new windows append.
    #[test]
    fn preserves_window_order_and_appends() {
        let mut state = State::default();
        let base = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d1", "a1"),
            ],
        };
        state.reconcile(&base);

        let mut grown = base.clone();
        grown.windows.push(window("w3", "o1", "d1", "a1"));
        state.reconcile(&grown);

        assert_eq!(
            state.cells()[&key("o1", "d1", "a1")].windows,
            vec!["w1".into(), "w2".into(), "w3".into()]
        );
    }

    /// A window that disappears from the topology is pruned from its cell.
    #[test]
    fn prunes_dead_window_refs() {
        let mut state = State::default();
        let base = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d1", "a1"),
            ],
        };
        state.reconcile(&base);

        let mut shrunk = base.clone();
        shrunk.windows.retain(|w| w.id == "w1".into());
        let report = state.reconcile(&shrunk);

        assert_eq!(report.windows, 1);
        assert_eq!(
            state.cells()[&key("o1", "d1", "a1")].windows,
            vec!["w1".into()]
        );
    }

    /// REGRESSION: an orphaned cell (output removed) cannot survive a reconcile.
    /// This is the exact failure the project exists to eliminate.
    #[test]
    fn orphaned_cell_cannot_survive_reconcile() {
        let mut state = State::default();
        let with_two = Topology {
            outputs: vec![output("o1"), output("o2")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o2", "d1", "a1"),
            ],
        };
        state.reconcile(&with_two);
        assert_eq!(state.cell_count(), 2);

        // o2 is unplugged: its window references a vanished output and the cell
        // keyed on o2 must be dropped, not stranded.
        let one_output = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o2", "d1", "a1"),
            ],
        };
        let report = state.reconcile(&one_output);

        assert_eq!(report.cells, 1, "cell on the removed output must be gone");
        assert!(state.cells().get(&key("o2", "d1", "a1")).is_none());
        assert!(state.cells().contains_key(&key("o1", "d1", "a1")));
    }

    /// A desktop switch / removal cannot strand a cell keyed on the old desktop.
    #[test]
    fn cell_on_removed_desktop_is_dropped() {
        let mut state = State::default();
        let two_desktops = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1"), desktop("d2")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d2", "a1"),
            ],
        };
        state.reconcile(&two_desktops);
        assert_eq!(state.cell_count(), 2);

        let one_desktop = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        };
        let report = state.reconcile(&one_desktop);
        assert_eq!(report.cells, 1);
        assert!(state.cells().get(&key("o1", "d2", "a1")).is_none());
    }

    /// `reset` rebuilds an identical cell map from the last topology.
    #[test]
    fn reset_rebuilds_from_last_topology() {
        let mut state = State::default();
        let topo = Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d1", "a1"),
            ],
        };
        state.reconcile(&topo);
        let before = state.report();
        let after = state.reset();
        assert_eq!(before, after);
        assert_eq!(state.cells()[&key("o1", "d1", "a1")].windows.len(), 2);
    }

    /// A two-window cell on which we can exercise the control commands.
    fn two_window_state() -> State {
        let mut state = State::default();
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d1", "a1"),
            ],
        });
        state
    }

    /// Directional focus is resolved from the live layout geometry.
    #[test]
    fn focus_neighbor_uses_geometry() {
        let mut state = two_window_state();
        state.set_focus(Some("w1".into()));
        // Default tile: w1 is the master (left), w2 the stack (right).
        assert_eq!(state.focus_neighbor(Direction::Right), Some("w2".into()));
        assert_eq!(state.focus_neighbor(Direction::Left), None);
    }

    /// Moving the focused window swaps it with its neighbor in the cell order.
    #[test]
    fn move_window_swaps_with_neighbor() {
        let mut state = two_window_state();
        state.set_focus(Some("w1".into()));
        assert_eq!(state.move_window(Direction::Right), MoveOutcome::Swapped);
        assert_eq!(
            state.cells()[&key("o1", "d1", "a1")].windows,
            vec!["w2".into(), "w1".into()]
        );
    }

    /// Switching layout affects the focused window's cell.
    #[test]
    fn set_layout_switches_focused_cell() {
        let mut state = two_window_state();
        state.set_focus(Some("w1".into()));
        assert!(state.set_layout(LayoutKind::Monocle));
        assert_eq!(
            state.cells()[&key("o1", "d1", "a1")].layout,
            LayoutKind::Monocle
        );
    }

    /// Control commands no-op safely when nothing is focused.
    #[test]
    fn control_without_focus_is_noop() {
        let mut state = two_window_state();
        assert_eq!(state.focus_neighbor(Direction::Right), None);
        assert_eq!(state.move_window(Direction::Right), MoveOutcome::None);
        assert!(!state.set_layout(LayoutKind::Monocle));
    }

    /// Master count and ratio adjust within their clamped bounds.
    #[test]
    fn master_adjustments_clamp() {
        let mut state = State::default();
        state.adjust_master_count(-5);
        assert_eq!(state.params.master_count, 1);
        state.adjust_master_count(2);
        assert_eq!(state.params.master_count, 3);

        state.adjust_master_ratio(1.0);
        assert!(state.params.master_ratio <= 0.95);
        state.adjust_master_ratio(-2.0);
        assert!(state.params.master_ratio >= 0.05);
    }

    /// Applying a config updates params, default layout, and behavior; a newly
    /// materialized cell adopts the new default and geometry reflects new gaps.
    #[test]
    fn apply_config_updates_params_and_default() {
        use crate::config::{BehaviorConfig, Config, GapsConfig, LayoutConfig};

        let mut state = State::default();
        let cfg = Config {
            layout: LayoutConfig {
                default: LayoutKind::Monocle,
                master_ratio: 0.7,
                master_count: 3,
                tiling_enabled: true,
            },
            gaps: GapsConfig { inner: 0, outer: 0 },
            behavior: BehaviorConfig {
                per_desktop: false,
                per_activity: false,
                focus_follows_mouse: true,
            },
            rules: Vec::new(),
            keys: HashMap::new(),
        };
        state.apply_config(&cfg);

        assert_eq!(state.params.master_ratio, 0.7);
        assert_eq!(state.params.master_count, 3);
        assert_eq!(state.params.gaps_inner, 0);
        assert_eq!(state.params.gaps_outer, 0);
        assert!(!state.behavior.per_desktop);
        assert!(state.behavior.focus_follows_mouse);

        // A newly materialized cell adopts the new default layout. Both opt-in
        // flags are off in this config, so the cell is keyed on the output alone.
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });
        assert_eq!(
            state.cells()[&collapsed_key("o1")].layout,
            LayoutKind::Monocle
        );

        // With zero gaps, the lone window fills the whole output.
        let geom = state.geometry();
        assert_eq!(geom.windows.len(), 1);
        assert_eq!(
            geom.windows[0].rect,
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }
        );
    }

    /// The config report mirrors the applied config and the source metadata.
    #[test]
    fn config_report_mirrors_applied_config() {
        use crate::config::{BehaviorConfig, Config, GapsConfig, LayoutConfig};

        let mut state = State::default();
        state.apply_config(&Config {
            layout: LayoutConfig {
                default: LayoutKind::Spiral,
                master_ratio: 0.5,
                master_count: 2,
                tiling_enabled: true,
            },
            gaps: GapsConfig {
                inner: 4,
                outer: 16,
            },
            behavior: BehaviorConfig::default(),
            rules: Vec::new(),
            keys: HashMap::new(),
        });
        let report = state.config_report("/tmp/riftrc".into(), true);
        assert_eq!(report.layout, LayoutKind::Spiral);
        assert_eq!(report.master_ratio, 0.5);
        assert_eq!(report.master_count, 2);
        assert_eq!(report.gaps_inner, 4);
        assert_eq!(report.gaps_outer, 16);
        assert_eq!(report.source, "/tmp/riftrc");
        assert!(report.loaded);
    }

    /// Tiling defaults on; toggling it off empties the geometry, and toggling
    /// it back on re-tiles.
    #[test]
    fn toggle_tiling_gates_geometry() {
        let mut state = two_window_state();
        assert!(!state.geometry().windows.is_empty());

        assert!(!state.toggle_tiling()); // now disabled
        assert!(state.geometry().windows.is_empty());

        assert!(state.toggle_tiling()); // re-enabled
        assert_eq!(state.geometry().windows.len(), 2);
    }

    /// Floating the focused window removes it from the layout; the rest re-tile.
    #[test]
    fn toggle_float_excludes_window_from_layout() {
        let mut state = two_window_state();
        state.set_focus(Some("w1".into()));

        assert_eq!(state.toggle_float(None), Some(true));
        let geom = state.geometry();
        assert!(geom.windows.iter().all(|g| g.id != "w1".into()));
        // The single remaining tiled window fills the cell.
        assert_eq!(geom.windows.len(), 1);
        assert_eq!(geom.windows[0].id, "w2".into());

        // Toggling again un-floats it.
        assert_eq!(state.toggle_float(Some("w1".into())), Some(false));
        assert_eq!(state.geometry().windows.len(), 2);
    }

    /// `toggle_float(None)` with nothing focused resolves no target.
    #[test]
    fn toggle_float_without_focus_is_none() {
        let mut state = two_window_state();
        assert_eq!(state.toggle_float(None), None);
    }

    /// A floated window that closes is pruned from the floating set on reconcile.
    #[test]
    fn reconcile_prunes_closed_floating_window() {
        let mut state = two_window_state();
        assert_eq!(state.toggle_float(Some("w2".into())), Some(true));

        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });

        // w2 is gone; re-adding it must start un-floated (clean prune).
        assert!(!state.floating_windows.contains(&"w2".into()));
    }

    /// A transient empty topology (resume/hotplug blip) is ignored: the cell map
    /// and geometry survive, and the next non-empty topology relayouts.
    #[test]
    fn empty_topology_preserves_prior_cells() {
        let mut state = two_window_state();
        let before = state.geometry().windows.len();
        assert_eq!(before, 2);

        // Zero outputs mid-reconfigure: held, not wiped.
        let report = state.reconcile(&Topology::default());
        assert_eq!(report.cells, 1, "prior cell is held across the empty blip");
        assert_eq!(report.windows, 2);
        assert_eq!(state.geometry().windows.len(), 2, "geometry survives");

        // Outputs return: the layout reconciles normally again.
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });
        assert_eq!(
            state.geometry().windows.len(),
            1,
            "relayouts after recovery"
        );
    }

    /// `apply_config` carries `tiling_enabled` through to the running state.
    #[test]
    fn apply_config_sets_tiling_enabled() {
        use crate::config::{BehaviorConfig, Config, GapsConfig, LayoutConfig};

        let mut state = two_window_state();
        state.apply_config(&Config {
            layout: LayoutConfig {
                default: LayoutKind::Tile,
                master_ratio: 0.6,
                master_count: 1,
                tiling_enabled: false,
            },
            gaps: GapsConfig::default(),
            behavior: BehaviorConfig::default(),
            rules: Vec::new(),
            keys: HashMap::new(),
        });
        assert!(state.geometry().windows.is_empty());
    }

    /// Build a state with the given opt-in flags applied via config.
    fn state_with_behavior(per_desktop: bool, per_activity: bool) -> State {
        use crate::config::{BehaviorConfig, Config, GapsConfig, LayoutConfig};

        let mut state = State::default();
        state.apply_config(&Config {
            layout: LayoutConfig {
                default: LayoutKind::Tile,
                master_ratio: 0.6,
                master_count: 1,
                tiling_enabled: true,
            },
            gaps: GapsConfig::default(),
            behavior: BehaviorConfig {
                per_desktop,
                per_activity,
                focus_follows_mouse: false,
            },
            rules: Vec::new(),
            keys: HashMap::new(),
        });
        state
    }

    /// The cell key for an output whose desktop+activity dimensions are collapsed.
    fn collapsed_key(output: &str) -> CellKey {
        CellKey {
            output: output.into(),
            desktop: COLLAPSED.into(),
            activity: COLLAPSED.into(),
        }
    }

    /// `per_activity = false` merges windows across activities into one cell.
    #[test]
    fn per_activity_off_merges_activities_into_one_cell() {
        let mut state = state_with_behavior(true, false);
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1"), activity("a2")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d1", "a2"),
            ],
        });
        assert_eq!(state.cell_count(), 1, "activities collapse into one cell");
        assert_eq!(state.window_count(), 2);
    }

    /// `per_desktop = false` merges windows across desktops into one cell.
    #[test]
    fn per_desktop_off_merges_desktops_into_one_cell() {
        let mut state = state_with_behavior(false, true);
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1"), desktop("d2")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d2", "a1"),
            ],
        });
        assert_eq!(state.cell_count(), 1, "desktops collapse into one cell");
        assert_eq!(state.window_count(), 2);
    }

    /// With both flags off, only the output keys a cell; outputs never collapse.
    #[test]
    fn both_flags_off_keys_cells_by_output_only() {
        let mut state = state_with_behavior(false, false);
        state.reconcile(&Topology {
            outputs: vec![output("o1"), output("o2")],
            desktops: vec![desktop("d1"), desktop("d2")],
            activities: vec![activity("a1"), activity("a2")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                window("w2", "o1", "d2", "a2"),
                window("w3", "o2", "d1", "a1"),
            ],
        });
        // o1's two windows (different desktop *and* activity) share one cell; o2
        // keeps its own. Output is never collapsed.
        assert_eq!(state.cell_count(), 2);
        assert_eq!(state.cells()[&collapsed_key("o1")].windows.len(), 2);
        assert_eq!(state.cells()[&collapsed_key("o2")].windows.len(), 1);
    }

    /// Collapsing a dimension must not weaken orphan pruning: a window on a
    /// vanished desktop is still dropped even when `per_desktop` is off.
    #[test]
    fn collapsed_desktop_still_drops_orphan_window() {
        let mut state = state_with_behavior(false, true);
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window("w1", "o1", "d1", "a1"),
                // d2 is not a live desktop -> orphan, dropped despite collapse.
                window("w2", "o1", "d2", "a1"),
            ],
        });
        assert_eq!(state.window_count(), 1);
    }

    /// A window matching a `float` rule is excluded from tiling; others tile.
    #[test]
    fn float_rule_excludes_matched_window_from_layout() {
        use crate::config::{BehaviorConfig, Config, GapsConfig, LayoutConfig, WindowRule};

        let mut state = State::default();
        state.apply_config(&Config {
            layout: LayoutConfig {
                default: LayoutKind::Tile,
                master_ratio: 0.6,
                master_count: 1,
                tiling_enabled: true,
            },
            gaps: GapsConfig::default(),
            behavior: BehaviorConfig::default(),
            rules: vec![WindowRule {
                class: Some("pavucontrol".into()),
                title: None,
                float: true,
            }],
            keys: HashMap::new(),
        });
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![
                window_classed("w1", "o1", "konsole"),
                window_classed("w2", "o1", "pavucontrol"),
            ],
        });

        // Both windows are managed (in the cell), but only w1 gets geometry.
        assert_eq!(state.window_count(), 2);
        let geom = state.geometry();
        assert_eq!(geom.windows.len(), 1);
        assert_eq!(geom.windows[0].id, "w1".into());
    }

    /// A window at its cell's edge moves onto the spatially adjacent output,
    /// landing in that output's cell with geometry inside its rectangle.
    #[test]
    fn move_at_cell_edge_crosses_to_adjacent_output() {
        let mut state = State::default();
        state.reconcile(&Topology {
            outputs: vec![output_at("o1", 0), output_at("o2", 1920)],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });
        state.set_focus(Some("w1".into()));

        // The lone window has no in-cell neighbor; moving right crosses to o2.
        assert_eq!(
            state.move_window(Direction::Right),
            MoveOutcome::CrossedOutput
        );

        // w1 now lives in o2's cell and lays out within o2's rectangle.
        assert!(
            state.cells()[&key("o2", "d1", "a1")]
                .windows
                .contains(&"w1".into())
        );
        let geom = state.geometry();
        let wg = geom
            .windows
            .iter()
            .find(|g| g.id == "w1".into())
            .expect("moved window has geometry");
        assert!(wg.rect.x >= 1920, "window placed on the right output");
    }

    /// A move toward an edge with no output that way leaves the window put.
    #[test]
    fn move_at_edge_without_adjacent_output_is_noop() {
        let mut state = State::default();
        state.reconcile(&Topology {
            outputs: vec![output_at("o1", 0), output_at("o2", 1920)],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });
        state.set_focus(Some("w1".into()));
        // Nothing lies left of o1.
        assert_eq!(state.move_window(Direction::Left), MoveOutcome::None);
        assert!(
            state.cells()[&key("o1", "d1", "a1")]
                .windows
                .contains(&"w1".into())
        );
    }

    /// Resize Left/Right move the master ratio; Up/Down are reserved no-ops.
    #[test]
    fn resize_adjusts_master_ratio_left_right_only() {
        let mut state = State::default();
        let base = state.params.master_ratio;

        assert!(state.resize(Direction::Right));
        assert!(state.params.master_ratio > base);
        let grown = state.params.master_ratio;

        assert!(state.resize(Direction::Left));
        assert!(state.params.master_ratio < grown);

        let before = state.params.master_ratio;
        assert!(!state.resize(Direction::Up));
        assert!(!state.resize(Direction::Down));
        assert_eq!(
            state.params.master_ratio, before,
            "vertical resize is reserved"
        );
    }

    /// `[keys]` overrides replace a binding's key by id; others keep defaults.
    #[test]
    fn keybindings_apply_config_overrides() {
        use crate::config::{BehaviorConfig, Config, GapsConfig, LayoutConfig};

        let mut state = State::default();
        // Defaults out of the box.
        let default_left = state
            .keybindings()
            .into_iter()
            .find(|b| b.id == "rift_focus_left")
            .unwrap()
            .key;
        assert_eq!(default_left, "Meta+H");

        let mut keys = HashMap::new();
        keys.insert("rift_focus_left".to_string(), "Meta+Left".to_string());
        state.apply_config(&Config {
            layout: LayoutConfig {
                default: LayoutKind::Tile,
                master_ratio: 0.6,
                master_count: 1,
                tiling_enabled: true,
            },
            gaps: GapsConfig::default(),
            behavior: BehaviorConfig::default(),
            rules: Vec::new(),
            keys,
        });

        let table = state.keybindings();
        let left = table.iter().find(|b| b.id == "rift_focus_left").unwrap();
        assert_eq!(left.key, "Meta+Left", "override applied");
        let right = table.iter().find(|b| b.id == "rift_focus_right").unwrap();
        assert_eq!(right.key, "Meta+L", "untouched binding keeps its default");
    }

    /// Focus cannot survive its window being removed from the topology.
    #[test]
    fn focus_dropped_when_window_vanishes() {
        let mut state = two_window_state();
        state.set_focus(Some("w2".into()));
        assert_eq!(state.focused(), Some(&"w2".into()));

        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });
        assert_eq!(state.focused(), None);
    }
}
