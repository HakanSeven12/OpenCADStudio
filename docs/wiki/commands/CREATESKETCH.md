---
summary: Opens a sketch on a plane: sets the plane as the UCS, turns the view square-on and turns snapping on until you finish the sketch.
related: FINISHSKETCH, EXTRUDE, UCS, PLAN, BROWSER
---

## Usage
1. Type `CREATESKETCH` to sketch on the current UCS, or `CREATESKETCH XY`, `CREATESKETCH XZ` or `CREATESKETCH YZ` to sketch on an origin plane.
2. The view turns to look straight at the plane, and the command line reports that the sketch is open (Sketch1, Sketch2, ...).
3. Draw your profile with the normal drawing commands.
4. Run [`FINISHSKETCH`](FINISHSKETCH-Command) to close the sketch.

## Notes
- A sketch is a mode, not a separate object. What you draw is ordinary model-space geometry.
- When you finish, the previous UCS and snap setting come back, and the objects you drew become the Previous selection. [`EXTRUDE`](EXTRUDE-Command) with the Previous option can then use them.
- Only one sketch can be open at a time.
- Any other word after the command name prints the usage and does nothing.
