# Cell Model and Reconcile

Rift tiles within **cells**. A cell is the intersection of one output, one desktop,
and one activity — the smallest region that holds an independent tiling layout.

## Keys and cells

A cell is keyed by `(output, desktop, activity)`. Each cell owns:

- an ordered list of the windows currently placed in it,
- a layout kind (defaulting to the configured default),
- the layout parameters in effect (master ratio/count, gaps).

Cells are **derived from topology, never persisted**. The set of live cells is
whatever the most recent topology implies; a cell exists only while its output,
desktop, and activity all exist.

## Reconcile

`reconcile(topology)` brings the cell model in line with a fresh snapshot:

1. **Drop invalid windows** — any window referencing an output, desktop, or
   activity that no longer exists is discarded.
2. **Prune dead cells** — cells whose key is no longer present in the topology are
   removed.
3. **Prune departed windows** — windows missing from the snapshot are removed from
   their cells.
4. **Materialize new cells** — newly present `(output, desktop, activity)` tuples
   get a cell with the default layout.
5. **Place new windows** — windows not yet tracked are appended to their cell in
   topology order, keeping placement deterministic.

`reset()` rebuilds the entire model from the retained last topology, forcing full
re-materialization. This is the recovery path exposed by `riftctl reset`.

## Why this shape

Because cells are recomputed rather than stored, the model cannot accumulate stale
state across monitor hotplugs, desktop changes, or activity switches — the
reliability property the reconcile tests guard (an orphaned cell cannot survive a
reconcile).
