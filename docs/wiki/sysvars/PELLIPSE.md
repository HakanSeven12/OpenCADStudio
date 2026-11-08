---
summary: Sets whether the ELLIPSE command creates a true ellipse or a polyline.
type: On/Off (0/1)
saved: Drawing
default: 0 (True ellipse)
related: ELLIPSE
---

## Values
| Value | Meaning |
|---|---|
| 0 | [`ELLIPSE`](ELLIPSE-Command) creates a true ellipse object. |
| 1 | [`ELLIPSE`](ELLIPSE-Command) creates a polyline made of arcs that follows the ellipse. |

## Notes
- A full ellipse becomes a closed polyline of 16 arcs. An elliptical arc becomes an open polyline.
- Only new ellipses are affected. Existing ellipses do not change.
- Only 0 and 1 are accepted.
