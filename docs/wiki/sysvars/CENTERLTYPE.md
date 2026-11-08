---
summary: Sets the linetype of new center lines and center marks.
type: String
saved: Drawing
default: Empty (the CENTER2 linetype)
related: CENTERLTSCALE, CENTERLTYPEFILE, CENTERLAYER, CENTERLINE, CENTERMARK, CELTYPE
---

## Values
| Value | Meaning |
|---|---|
| (empty) | Use the CENTER2 linetype. |
| . | Use the current linetype. |
| ByLayer or ByBlock | Use that setting. |
| A linetype name | Use this linetype. It must be in the drawing or in the built-in linetype catalog; a catalog linetype is loaded into the drawing. |

## Notes
- An unknown linetype name is refused and asked for again.
- Used by [`CENTERLINE`](CENTERLINE-Command) and [`CENTERMARK`](CENTERMARK-Command).
