---
summary: Sets the record increment of SKETCH, the smallest distance between recorded points.
type: Real
saved: Drawing
default: 0.1
related: SKETCH, SKPOLY, SKTOLERANCE
---

## Values
Any positive number, in drawing units. Zero, negative and non-numeric values are refused.

## Notes
- [`SKETCH`](SKETCH-Command) records a new point only after the cursor has moved at least this far. Smaller values give smoother, denser sketches.
- Changing the increment inside SKETCH also updates this variable.
