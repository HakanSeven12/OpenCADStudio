---
summary: Sets the default inside diameter of new donuts.
type: Real
saved: Not saved
default: 0.5
related: DONUTOD, DONUT
---

## Values
Any finite number that is zero or larger. A value of 0 makes a filled circle.

## Notes
- [`DONUT`](DONUT-Command) offers this value as its first default.
- When you draw a donut, the diameters you enter become the new defaults.
- If the inside diameter is larger than the outside diameter, the two are swapped.
- The value is kept only for the current session.
