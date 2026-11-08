---
summary: Controls whether new center marks get extension lines.
type: On/Off (0/1)
saved: Drawing
default: 1 (On)
related: CENTEREXE, CENTERCROSSSIZE, CENTERCROSSGAP, CENTERMARK, CENTERRESET
---

## Values
| Value | Meaning |
|---|---|
| 0 (OFF) | New center marks show only the center cross. |
| 1 (ON) | New center marks also get extension lines. |

## Notes
- You can type 0, 1, ON or OFF.
- Used by [`CENTERMARK`](CENTERMARK-Command). [`CENTERRESET`](CENTERRESET-Command) applies the current value to existing center marks.
