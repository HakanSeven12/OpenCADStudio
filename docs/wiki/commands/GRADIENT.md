---
summary: Fills enclosed areas or selected closed objects with a one-color or two-color gradient.
related: HATCH, HATCHEDIT, BOUNDARY, HATCHTOBACK
ribbon: Draw tab › Draw panel
---

## Usage
`GRADIENT` works like [`HATCH`](HATCH-Command), with a gradient fill selected from the start.

1. Pick internal point or [Select objects/Draw/Undo/seTtings]: click inside each area you want to fill.
2. Pick more areas, or type an option.
3. Press Enter to accept and create the fill.

## Options
The options are the same as in [`HATCH`](HATCH-Command): **Select objects** (`S`), **picK internal point** (`K`), **Draw** (`D`), **Undo** (`U`) and **seTtings** (`T`).

## Notes
- Use the settings panel to choose the gradient shape, **Color 1** and **Color 2**, angle and the other fill settings.
- Unlike [`HATCH`](HATCH-Command), objects selected before you start are not used, and a selected hatch is not copied.
- Boundaries are searched in the world XY plane.
