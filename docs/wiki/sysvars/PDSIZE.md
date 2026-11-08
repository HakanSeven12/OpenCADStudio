---
summary: Sets the display size of point objects.
type: Real
saved: Drawing
default: 0 (5% of the viewport height)
related: PDMODE, DDPTYPE, PTYPE, POINT
---

## Values
| Value | Meaning |
|---|---|
| 0 | Points are 5% of the viewport height. The size stays the same on screen when you zoom. |
| Greater than 0 | An absolute size in drawing units. |
| Less than 0 | A percentage of the viewport height. For example, -10 means 10%. |

## Notes
- The size has no effect when [`PDMODE`](PDMODE-System-Variable) is 0 (dot).
- All points in the drawing change at once when you change the value.
- [`DDPTYPE`](DDPTYPE-Command) and [`PTYPE`](PTYPE-Command) set this value from a dialog.
