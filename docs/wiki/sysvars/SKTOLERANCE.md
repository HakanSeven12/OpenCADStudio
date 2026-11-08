---
summary: Sets how closely a sketched spline follows the sketched points.
type: Real
saved: Drawing
default: 0.5
related: SKETCH, SKPOLY, SKETCHINC
---

## Values
A number from 0 to 1. The allowed distance between the spline and the points is this value times [`SKETCHINC`](SKETCHINC-System-Variable). Smaller values follow the points more closely.

## Notes
- Used only when [`SKPOLY`](SKPOLY-System-Variable) is 2 (splines).
- Changing the tolerance inside [`SKETCH`](SKETCH-Command) also updates this variable.
