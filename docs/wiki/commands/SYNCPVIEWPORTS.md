---
summary: Copies the view, scale, snap, grid and frozen layers of the first selected layout viewport to the other selected viewports.
related: VPSYNC, MVIEW, VPLAYER
---

## Usage
1. Select two or more layout viewports. The first one you select is the master.
2. Type `SYNCPVIEWPORTS`.
3. The other viewports take the master's view direction, view target, view height (scale), snap base, spacing and angle, grid spacing, and frozen layer list.

## Notes
- Select the viewports before you start; the command has no selection prompt.
- With fewer than two viewports selected, you see `SYNCPVIEWPORTS: select two or more viewports (the first is the master).`
- Viewports on locked layers are skipped.
- The command line reports how many viewports were changed. The change can be undone.
