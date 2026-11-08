---
summary: Draws the edges of the selected 3D solids as 2D lines on the XY plane at Z=0.
related: FLATTEN, SECTIONPLANE, CONVTOSURFACE
---

## Usage
1. Select one or more 3D solids.
2. Type `FLATSHOT`.
3. Each solid edge is drawn as lines at Z=0. The command reports how many lines it created.

## Notes
- The edges are projected straight down onto the world XY plane (a top view). The current view direction is not used.
- The solids stay as they are. The new lines are added to the drawing.
- Solids on locked layers are skipped.
- If no solid is selected, you see `FLATSHOT: select a solid created this session.`
- The result is one undo step.
