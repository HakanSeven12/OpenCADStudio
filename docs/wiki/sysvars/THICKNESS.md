---
summary: Holds the default 3D thickness of new objects.
type: Real
saved: Drawing
default: 0
related: ELEVATION, RECTANG
---

Any number is accepted.

## Notes
- Stored with the drawing; it does not change anything in Open CAD Studio yet. New objects are not drawn with this thickness.
- The Properties panel shows it, read-only, when nothing is selected.
- When the drawing units are converted, the value is scaled with them.
- [`RECTANG`](RECTANG-Command) has its own Thickness option.
