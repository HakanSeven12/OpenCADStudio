---
summary: Controls how the coordinate display on the status bar is updated.
type: Integer
saved: Drawing
default: 2 (Polar while picking)
related: DYNMODE
---

## Values
| Value | Meaning |
|---|---|
| 0 | Static: shows the last picked point and changes only when you pick. |
| 1 | Live: shows the absolute cursor position all the time. |
| 2 | Like 1, but while a command asks for a point after a first point, it shows distance and angle from the last point. |

## Notes
- Click the coordinate display on the status bar to step through 0, 1 and 2.
- Other integers are accepted and behave like 1.
