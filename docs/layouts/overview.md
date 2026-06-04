# Layouts

A layout maps an ordered list of windows to rectangles within a cell's area. The
engine first insets the area by the outer gap, then reserves inner gaps between
tiles. All splits are integer with the remainder spread to the leading segments, so
tiles are gap-separated, disjoint, and stay within bounds.

Switch layouts at runtime with a keybinding (see
[../usage/keybindings.md](../usage/keybindings.md)) or `riftctl layout <kind>`.

## tile

Master/stack. The master area takes `master_ratio` of the width and holds
`master_count` windows; the remaining windows split the stack column evenly. When
every window fits in the master area, it collapses to a single column.

## monocle

Each window fills the entire cell area. Only the focused window is visible; this is
a full-screen stack.

## columns

All windows split the area into equal-width columns, left to right.

## spiral

Recursive fibonacci split: the area is halved alternately horizontally and
vertically, each window taking one half and the next window recursing into the
remainder. Tiles remain disjoint.

## threecolumn

A centered master column flanked by stacks split across both sides. With only one
side populated it degrades gracefully to a `tile`-style master/stack split.

## floating

No geometry is emitted; windows keep whatever position they have. Use this to opt a
cell out of tiling.

## Master controls

`tile`, `columns`, and `threecolumn` respond to the master adjustments:

- **master ratio** — clamped to `0.05`–`0.95`.
- **master count** — floored at `1`.

Both are adjustable live via keybindings or `riftctl master-ratio` /
`riftctl master-count`.
