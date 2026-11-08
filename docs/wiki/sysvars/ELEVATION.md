---
summary: Sets the current elevation, the height above the current UCS plane.
type: Real
saved: Drawing
default: 0.0
related: THICKNESS, OSNAPZ
---

## Values
Any number.

## Notes
- When [`OSNAPZ`](OSNAPZ-System-Variable) is on, object snaps are placed on the current UCS plane at this elevation.
- The value can also be read by fields and by the calculator.
- New objects are not raised to this elevation yet.
- The Elevation option of [`RECTANG`](RECTANG-Command) is a separate setting of that command.
