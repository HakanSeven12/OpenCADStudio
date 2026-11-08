---
summary: Sets how far new center lines and the extension lines of new center marks reach past the geometry.
type: Real
saved: Drawing
default: 0.12 in imperial drawings; 3.5 in metric drawings
related: CENTERMARKEXE, CENTERLINE, CENTERMARK, CENTERRESET
---

## Values
Any finite number, as a length in drawing units.

## Notes
- The default is 3.5 when the drawing uses metric measurement or metric insertion units; otherwise it is 0.12.
- Used by [`CENTERLINE`](CENTERLINE-Command) and [`CENTERMARK`](CENTERMARK-Command). [`CENTERRESET`](CENTERRESET-Command) sets the extensions of existing center lines and center marks back to this value.
