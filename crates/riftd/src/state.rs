//! Cell model and reconciliation.
//!
//! A *cell* is the layout state for one `(output, desktop, activity)` tuple.
//! Cells are **never the source of truth**: on every reconcile the cell map is
//! rebuilt from the live [`Topology`], so an orphaned cell — one whose output,
//! desktop, or activity has vanished — cannot survive. This is the failure mode
//! the project exists to eliminate.

use std::collections::{HashMap, HashSet};

use rift_ipc::{
    DesktopId, Direction, GeometrySet, LayoutKind, OutputId, ReconcileReport, Rect, Topology,
    WindowGeometry, WindowId,
};

use crate::config::{BehaviorConfig, Config};
use crate::layout::{self, LayoutParams};

/// Identifies a cell by its `(output, desktop, activity)` tuple.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub output: OutputId,
    pub desktop: DesktopId,
    pub activity: rift_ipc::ActivityId,
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
    /// Session behavior flags from config. Stored and surfaced; their runtime
    /// effects are deferred (see the milestone notes).
    behavior: BehaviorConfig,
    /// Whether global auto-tiling is enabled. When false, [`State::geometry`]
    /// emits nothing so every window floats where KWin last left it.
    tiling_enabled: bool,
    /// Windows the user has explicitly floated; the layout engine skips them.
    /// Pruned to the live topology on every reconcile.
    floating_windows: HashSet<WindowId>,
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
        let outputs: HashSet<&OutputId> = topology.outputs.iter().map(|o| &o.id).collect();
        let desktops: HashSet<&DesktopId> = topology.desktops.iter().map(|d| &d.id).collect();
        let activities: HashSet<&rift_ipc::ActivityId> =
            topology.activities.iter().map(|a| &a.id).collect();

        // (1) Map present tuples -> ordered windows, skipping orphan references.
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
                desktop: w.desktop.clone(),
                activity: w.activity.clone(),
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
            // Floated windows are excluded from the layout; the rest re-tile.
            let tiled: Vec<WindowId> = cell
                .windows
                .iter()
                .filter(|w| !self.floating_windows.contains(*w))
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

    /// Swap the focused window with its directional neighbor within the same
    /// cell, changing their layout positions. Returns whether anything moved.
    pub fn move_window(&mut self, direction: Direction) -> bool {
        let Some(focused) = self.focused.clone() else {
            return false;
        };
        let Some(key) = self.cell_of(&focused) else {
            return false;
        };
        // Restrict the neighbor search to this cell so a move stays local.
        let cell_ids: HashSet<WindowId> = self.cells[&key].windows.iter().cloned().collect();
        let local: Vec<WindowGeometry> = self
            .geometry()
            .windows
            .into_iter()
            .filter(|g| cell_ids.contains(&g.id))
            .collect();
        let Some(neighbor) = layout::neighbor(&local, &focused, direction) else {
            return false;
        };

        let cell = self.cells.get_mut(&key).expect("focused cell exists");
        let (Some(i), Some(j)) = (
            cell.windows.iter().position(|w| *w == focused),
            cell.windows.iter().position(|w| *w == neighbor),
        ) else {
            return false;
        };
        cell.windows.swap(i, j);
        true
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
    /// Behavior flags are stored but their runtime effects are deferred.
    pub fn apply_config(&mut self, cfg: &Config) {
        self.default_layout = cfg.layout.default;
        self.params.master_ratio = cfg.layout.master_ratio;
        self.params.master_count = cfg.layout.master_count;
        self.params.gaps_inner = cfg.gaps.inner;
        self.params.gaps_outer = cfg.gaps.outer;
        self.tiling_enabled = cfg.layout.tiling_enabled;
        self.behavior = cfg.behavior.clone();
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
        assert!(state.move_window(Direction::Right));
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
        assert!(!state.move_window(Direction::Right));
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
        };
        state.apply_config(&cfg);

        assert_eq!(state.params.master_ratio, 0.7);
        assert_eq!(state.params.master_count, 3);
        assert_eq!(state.params.gaps_inner, 0);
        assert_eq!(state.params.gaps_outer, 0);
        assert!(!state.behavior.per_desktop);
        assert!(state.behavior.focus_follows_mouse);

        // A newly materialized cell adopts the new default layout.
        state.reconcile(&Topology {
            outputs: vec![output("o1")],
            desktops: vec![desktop("d1")],
            activities: vec![activity("a1")],
            windows: vec![window("w1", "o1", "d1", "a1")],
        });
        assert_eq!(
            state.cells()[&key("o1", "d1", "a1")].layout,
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
        });
        assert!(state.geometry().windows.is_empty());
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
