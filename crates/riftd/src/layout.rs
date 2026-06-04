//! Pure geometry computation for the tiling layouts.
//!
//! Every function here is a total function from a window count and an area to a
//! list of rectangles, with no I/O and no compositor coupling. The daemon's
//! state pairs the rectangles back to window ids; the KWin script merely applies
//! them. Keeping this layer pure is what lets the layouts be unit-tested without
//! a running compositor.

use rift_ipc::{Direction, LayoutKind, Rect, WindowGeometry, WindowId};

/// Tunables shared by every layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
    /// Fraction of the area given to the master column/region (0.0..=1.0).
    pub master_ratio: f32,
    /// Number of windows placed in the master region.
    pub master_count: usize,
    /// Gap between adjacent tiles, in pixels.
    pub gaps_inner: i32,
    /// Gap between the tiles and the output edge, in pixels.
    pub gaps_outer: i32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            master_ratio: 0.6,
            master_count: 1,
            gaps_inner: 8,
            gaps_outer: 12,
        }
    }
}

/// Compute geometry for `windows` within `area` under `kind`.
///
/// The returned vector pairs each input window with its target rectangle, in
/// input order. [`LayoutKind::Floating`] yields an empty vector: floating cells
/// impose no geometry.
pub fn arrange(
    kind: LayoutKind,
    windows: &[WindowId],
    area: Rect,
    params: &LayoutParams,
) -> Vec<WindowGeometry> {
    let n = windows.len();
    let inner = inset(area, params.gaps_outer);
    let rects = match kind {
        LayoutKind::Tile => tile(n, inner, params),
        LayoutKind::Monocle => monocle(n, inner),
        LayoutKind::Columns => split_h(inner, n, params.gaps_inner),
        LayoutKind::Spiral => spiral(n, inner, params),
        LayoutKind::ThreeColumn => three_column(n, inner, params),
        LayoutKind::Floating => Vec::new(),
    };
    windows
        .iter()
        .cloned()
        .zip(rects)
        .map(|(id, rect)| WindowGeometry { id, rect })
        .collect()
}

/// Find the best window to move to when stepping `dir` from `focused`.
///
/// Candidates are scored from their rectangle centers: only windows whose center
/// lies in the requested direction are eligible, and among those the nearest
/// along that axis wins, with perpendicular drift penalized so the choice stays
/// roughly in line. Returns `None` when nothing lies that way. Because the rects
/// are in global coordinates this naturally crosses output boundaries.
pub fn neighbor(geoms: &[WindowGeometry], focused: &WindowId, dir: Direction) -> Option<WindowId> {
    let origin = geoms.iter().find(|g| &g.id == focused)?;
    let (fx, fy) = center(origin.rect);

    let mut best: Option<(i64, &WindowId)> = None;
    for g in geoms {
        if &g.id == focused {
            continue;
        }
        let (cx, cy) = center(g.rect);
        let (dx, dy) = (cx - fx, cy - fy);
        let in_dir = match dir {
            Direction::Left => dx < 0,
            Direction::Right => dx > 0,
            Direction::Up => dy < 0,
            Direction::Down => dy > 0,
        };
        if !in_dir {
            continue;
        }
        let (along, perp) = match dir {
            Direction::Left | Direction::Right => (dx.unsigned_abs(), dy.unsigned_abs()),
            Direction::Up | Direction::Down => (dy.unsigned_abs(), dx.unsigned_abs()),
        };
        let score = along as i64 + 2 * perp as i64;
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, &g.id));
        }
    }
    best.map(|(_, id)| id.clone())
}

/// Integer center point of a rectangle.
fn center(r: Rect) -> (i32, i32) {
    (r.x + r.width / 2, r.y + r.height / 2)
}

/// Shrink a rectangle inward by `by` pixels on every side.
fn inset(r: Rect, by: i32) -> Rect {
    Rect {
        x: r.x + by,
        y: r.y + by,
        width: (r.width - 2 * by).max(0),
        height: (r.height - 2 * by).max(0),
    }
}

/// Split `total` into `n` integer lengths separated by `n - 1` gaps,
/// distributing any rounding remainder to the leading segments.
fn split_lengths(total: i32, n: usize, gap: i32) -> Vec<i32> {
    if n == 0 {
        return Vec::new();
    }
    let n_i = n as i32;
    let avail = (total - gap * (n_i - 1)).max(0);
    let base = avail / n_i;
    let rem = avail % n_i;
    (0..n)
        .map(|i| base + if (i as i32) < rem { 1 } else { 0 })
        .collect()
}

/// Split `area` into `n` stacked rows, top to bottom, separated by `gap`.
fn split_v(area: Rect, n: usize, gap: i32) -> Vec<Rect> {
    let mut y = area.y;
    split_lengths(area.height, n, gap)
        .into_iter()
        .map(|h| {
            let r = Rect {
                x: area.x,
                y,
                width: area.width,
                height: h,
            };
            y += h + gap;
            r
        })
        .collect()
}

/// Split `area` into `n` side-by-side columns, left to right, separated by `gap`.
fn split_h(area: Rect, n: usize, gap: i32) -> Vec<Rect> {
    let mut x = area.x;
    split_lengths(area.width, n, gap)
        .into_iter()
        .map(|w| {
            let r = Rect {
                x,
                y: area.y,
                width: w,
                height: area.height,
            };
            x += w + gap;
            r
        })
        .collect()
}

/// Split `area` into a left/right pair at `ratio`, reserving one inner `gap`.
fn split_columns(area: Rect, ratio: f32, gap: i32) -> (Rect, Rect) {
    let avail = (area.width - gap).max(0);
    let left_w = (avail as f32 * ratio).round() as i32;
    let right_w = avail - left_w;
    let left = Rect {
        x: area.x,
        y: area.y,
        width: left_w,
        height: area.height,
    };
    let right = Rect {
        x: area.x + left_w + gap,
        y: area.y,
        width: right_w,
        height: area.height,
    };
    (left, right)
}

/// Split `area` into a top/bottom pair at `ratio`, reserving one inner `gap`.
fn split_rows(area: Rect, ratio: f32, gap: i32) -> (Rect, Rect) {
    let avail = (area.height - gap).max(0);
    let top_h = (avail as f32 * ratio).round() as i32;
    let bottom_h = avail - top_h;
    let top = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: top_h,
    };
    let bottom = Rect {
        x: area.x,
        y: area.y + top_h + gap,
        width: area.width,
        height: bottom_h,
    };
    (top, bottom)
}

/// Master/stack: `master_count` windows tile the master column, the rest tile
/// the stack column. Collapses to a single vertical column when every window
/// fits in the master.
fn tile(n: usize, area: Rect, params: &LayoutParams) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let mc = params.master_count.clamp(1, n);
    if n <= mc {
        return split_v(area, n, params.gaps_inner);
    }
    let (master_area, stack_area) = split_columns(area, params.master_ratio, params.gaps_inner);
    let mut rects = split_v(master_area, mc, params.gaps_inner);
    rects.extend(split_v(stack_area, n - mc, params.gaps_inner));
    rects
}

/// Monocle: every window occupies the full area, stacked.
fn monocle(n: usize, area: Rect) -> Vec<Rect> {
    vec![area; n]
}

/// Spiral (fibonacci): each window claims a fraction of the remaining area,
/// alternating between vertical and horizontal splits.
fn spiral(n: usize, area: Rect, params: &LayoutParams) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let mut rects = Vec::with_capacity(n);
    let mut remaining = area;
    for i in 0..n {
        if i + 1 == n {
            rects.push(remaining);
            break;
        }
        if i % 2 == 0 {
            let (head, rest) = split_columns(remaining, params.master_ratio, params.gaps_inner);
            rects.push(head);
            remaining = rest;
        } else {
            let (head, rest) = split_rows(remaining, params.master_ratio, params.gaps_inner);
            rects.push(head);
            remaining = rest;
        }
    }
    rects
}

/// Three-column: masters in the center, stack windows split between the left and
/// right columns (extras favor the left). Degrades to master/stack tiling when a
/// side would be empty, and to a single column when every window is a master.
fn three_column(n: usize, area: Rect, params: &LayoutParams) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let mc = params.master_count.clamp(1, n);
    if n <= mc {
        return split_v(area, n, params.gaps_inner);
    }
    let stack = n - mc;
    let left_count = stack.div_ceil(2);
    let right_count = stack - left_count;
    if right_count == 0 {
        // Only one side populated: fall back to master/stack tiling.
        return tile(n, area, params);
    }

    let gap = params.gaps_inner;
    let avail = (area.width - 2 * gap).max(0);
    let master_w = (avail as f32 * params.master_ratio).round() as i32;
    let side_total = avail - master_w;
    let left_w = side_total / 2;
    let right_w = side_total - left_w;

    let left_area = Rect {
        x: area.x,
        width: left_w,
        ..area
    };
    let master_area = Rect {
        x: area.x + left_w + gap,
        width: master_w,
        ..area
    };
    let right_area = Rect {
        x: area.x + left_w + gap + master_w + gap,
        width: right_w,
        ..area
    };

    // Emit in window order: masters, then left stack, then right stack.
    let mut rects = split_v(master_area, mc, gap);
    rects.extend(split_v(left_area, left_count, gap));
    rects.extend(split_v(right_area, right_count, gap));
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 1000,
        height: 1000,
    };

    fn ids(n: usize) -> Vec<WindowId> {
        (0..n).map(|i| WindowId::from(&*format!("w{i}"))).collect()
    }

    fn params() -> LayoutParams {
        // Zero gaps keep the arithmetic exact and assertions readable.
        LayoutParams {
            master_ratio: 0.6,
            master_count: 1,
            gaps_inner: 0,
            gaps_outer: 0,
        }
    }

    /// Floating imposes no geometry at all.
    #[test]
    fn floating_emits_nothing() {
        let g = arrange(LayoutKind::Floating, &ids(3), AREA, &params());
        assert!(g.is_empty());
    }

    /// Monocle stacks every window over the full area.
    #[test]
    fn monocle_fills_area_for_each() {
        let g = arrange(LayoutKind::Monocle, &ids(3), AREA, &params());
        assert_eq!(g.len(), 3);
        for wg in g {
            assert_eq!(wg.rect, AREA);
        }
    }

    /// A lone window fills the whole area regardless of layout.
    #[test]
    fn single_window_fills_area() {
        for kind in [
            LayoutKind::Tile,
            LayoutKind::Columns,
            LayoutKind::Spiral,
            LayoutKind::ThreeColumn,
        ] {
            let g = arrange(kind, &ids(1), AREA, &params());
            assert_eq!(g.len(), 1, "{kind:?} should place one window");
            assert_eq!(g[0].rect, AREA, "{kind:?} single window should fill area");
        }
    }

    /// Tile: master takes the ratio share on the left, the rest stack on the right.
    #[test]
    fn tile_master_stack_split() {
        let g = arrange(LayoutKind::Tile, &ids(3), AREA, &params());
        // master: 60% width, full height.
        assert_eq!(
            g[0].rect,
            Rect {
                x: 0,
                y: 0,
                width: 600,
                height: 1000
            }
        );
        // two stack windows: 40% width, half height each.
        assert_eq!(
            g[1].rect,
            Rect {
                x: 600,
                y: 0,
                width: 400,
                height: 500
            }
        );
        assert_eq!(
            g[2].rect,
            Rect {
                x: 600,
                y: 500,
                width: 400,
                height: 500
            }
        );
    }

    /// Columns: equal-width side-by-side columns covering the area.
    #[test]
    fn columns_are_equal_width() {
        let g = arrange(LayoutKind::Columns, &ids(4), AREA, &params());
        assert_eq!(g.len(), 4);
        for (i, wg) in g.iter().enumerate() {
            assert_eq!(wg.rect.width, 250);
            assert_eq!(wg.rect.height, 1000);
            assert_eq!(wg.rect.x, 250 * i as i32);
        }
    }

    /// Tiles never overlap and stay within the area (gaps on, multiple windows).
    #[test]
    fn tile_is_disjoint_and_bounded() {
        let p = LayoutParams {
            gaps_inner: 8,
            gaps_outer: 12,
            ..params()
        };
        let g = arrange(LayoutKind::Tile, &ids(4), AREA, &p);
        for wg in &g {
            assert!(wg.rect.x >= AREA.x);
            assert!(wg.rect.y >= AREA.y);
            assert!(wg.rect.x + wg.rect.width <= AREA.x + AREA.width);
            assert!(wg.rect.y + wg.rect.height <= AREA.y + AREA.height);
        }
        for (i, a) in g.iter().enumerate() {
            for b in g.iter().skip(i + 1) {
                assert!(
                    disjoint(a.rect, b.rect),
                    "{:?} overlaps {:?}",
                    a.rect,
                    b.rect
                );
            }
        }
    }

    /// Three-column: masters centered, stack split onto both flanks.
    #[test]
    fn three_column_centers_master() {
        // 1 master + 4 stack -> left 2, right 2.
        let g = arrange(LayoutKind::ThreeColumn, &ids(5), AREA, &params());
        let master = g[0].rect;
        // master is the 60% center column.
        assert_eq!(master.width, 600);
        assert_eq!(master.x, 200);
        // left column starts at the origin, right column past the master.
        assert_eq!(g[1].rect.x, 0);
        assert_eq!(g[3].rect.x, 800);
    }

    /// Spiral: first window keeps the master share; tiles stay disjoint.
    #[test]
    fn spiral_first_is_master_and_disjoint() {
        let g = arrange(LayoutKind::Spiral, &ids(4), AREA, &params());
        assert_eq!(
            g[0].rect,
            Rect {
                x: 0,
                y: 0,
                width: 600,
                height: 1000
            }
        );
        for (i, a) in g.iter().enumerate() {
            for b in g.iter().skip(i + 1) {
                assert!(
                    disjoint(a.rect, b.rect),
                    "{:?} overlaps {:?}",
                    a.rect,
                    b.rect
                );
            }
        }
    }

    fn disjoint(a: Rect, b: Rect) -> bool {
        let ax2 = a.x + a.width;
        let ay2 = a.y + a.height;
        let bx2 = b.x + b.width;
        let by2 = b.y + b.height;
        ax2 <= b.x || bx2 <= a.x || ay2 <= b.y || by2 <= a.y
    }

    fn wg(id: &str, x: i32, y: i32, w: i32, h: i32) -> WindowGeometry {
        WindowGeometry {
            id: WindowId::from(id),
            rect: Rect {
                x,
                y,
                width: w,
                height: h,
            },
        }
    }

    /// Neighbor lookup resolves the spatially nearest window per direction.
    #[test]
    fn neighbor_resolves_by_direction() {
        // a (left) | b (right), with c directly below a.
        let geoms = vec![
            wg("a", 0, 0, 100, 100),
            wg("b", 200, 0, 100, 100),
            wg("c", 0, 200, 100, 100),
        ];
        let a = WindowId::from("a");
        assert_eq!(
            neighbor(&geoms, &a, Direction::Right),
            Some(WindowId::from("b"))
        );
        assert_eq!(
            neighbor(&geoms, &a, Direction::Down),
            Some(WindowId::from("c"))
        );
        assert_eq!(neighbor(&geoms, &a, Direction::Up), None);
        assert_eq!(neighbor(&geoms, &a, Direction::Left), None);
        assert_eq!(
            neighbor(&geoms, &WindowId::from("b"), Direction::Left),
            Some(WindowId::from("a"))
        );
    }

    /// Neighbors cross output boundaries because rects are global.
    #[test]
    fn neighbor_crosses_outputs() {
        // Two windows on adjacent 1920-wide outputs.
        let geoms = vec![
            wg("left", 0, 0, 1920, 1080),
            wg("right", 1920, 0, 1920, 1080),
        ];
        assert_eq!(
            neighbor(&geoms, &WindowId::from("left"), Direction::Right),
            Some(WindowId::from("right"))
        );
    }

    /// A missing focus id yields no neighbor rather than panicking.
    #[test]
    fn neighbor_unknown_focus_is_none() {
        let geoms = vec![wg("a", 0, 0, 100, 100)];
        assert_eq!(
            neighbor(&geoms, &WindowId::from("ghost"), Direction::Right),
            None
        );
    }
}
