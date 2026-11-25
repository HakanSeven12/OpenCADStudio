---
summary: Sets the rotation angle of snap, grid, crosshair and Ortho directions in the current UCS.
type: Real
saved: Program settings
default: 0 (degrees)
related: DSETTINGS, ORTHOMODE
---

## Values
Any number of degrees. The value is wrapped into the range 0 to 360.

## Notes
- The crosshair, the grid, snap and Ortho all turn by this angle inside the current UCS plane.
- Typing `SNAPANG` asks for the angle: type a number, or pick two points and the direction from the first to the second becomes the angle, so the crosshair lines up with an existing line. Enter keeps the current angle.
- [`DSETTINGS`](DSETTINGS-Command) shows the current rotation and has a button that resets it to 0.
