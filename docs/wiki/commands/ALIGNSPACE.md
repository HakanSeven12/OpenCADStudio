---
summary: Pans, zooms and rotates a layout viewport's view so points in the model line up with points on the sheet.
related: MSPACE, PSPACE, VPSCALE, CHSPACE
---

## Usage
1. Switch to a layout tab.
2. Run `ALIGNSPACE`. It works in the active viewport, or a selected one, or the layout's first viewport, and switches into it.
3. Pick the first alignment point in model space, then a second one, or press Enter to align by one point.
4. The command switches back to paper space. Pick the sheet point for each model point, in the same order.
5. The viewport's view moves so the model points sit on the sheet points.

## Notes
- With one point, the view only pans; its scale and rotation stay.
- With two points, the view also takes the scale and the twist that carry the model segment onto the sheet segment.
- The viewport must show a plan view and must not be display-locked.
- The command does not run in the Model tab.
- UNDO takes the alignment back in one step.
