---
summary: Drawing default for how linetype patterns run around the vertices of 2D polylines.
type: On/Off (0/1)
saved: Drawing
default: 0 (Off)
related: PEDIT, PLINE
---

## Values
| Value | Meaning |
|---|---|
| 0 | The linetype pattern starts again at each vertex. |
| 1 | The linetype pattern runs on through the vertices of the whole polyline. |

## Notes
- Stored with the drawing; it does not change anything in Open CAD Studio yet. New polylines do not take their setting from it.
- To change a polyline, use the Linetype generation property in [`PROPERTIES`](PROPERTIES-Command) or the Ltype gen option of [`PEDIT`](PEDIT-Command).
