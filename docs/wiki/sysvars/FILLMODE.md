---
summary: Controls whether hatches, solid fills and wide polylines are drawn filled.
type: On/Off (0/1)
saved: Drawing
default: 1 (On)
related: TEXTFILL, HATCH
---

## Values
| Value | Meaning |
|---|---|
| 0 | Hatches, wipeouts and 2D solids are not filled, 3D faces are not filled, and wide polylines are drawn as outlines. |
| 1 | These objects are drawn filled. |

## Notes
- The drawing is redrawn at once when the value changes.
- SETVAR also accepts ON/OFF and TRUE/FALSE.
