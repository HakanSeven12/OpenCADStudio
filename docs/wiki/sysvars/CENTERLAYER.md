---
summary: Sets the layer that new center lines and center marks are placed on.
type: String
saved: Drawing
default: . (use the current layer)
related: CENTERLTYPE, CENTERLTSCALE, CENTERLINE, CENTERMARK, CLAYER
---

## Values
| Value | Meaning |
|---|---|
| . | Use the current layer. |
| A layer name | Use this layer. If the drawing does not have it yet, it is created. |

## Notes
- An empty value is refused.
- Used by [`CENTERLINE`](CENTERLINE-Command) and [`CENTERMARK`](CENTERMARK-Command). The center layer control on the ribbon shows the same value.
