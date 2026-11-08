---
summary: Controls whether object snaps use the Z value of the snapped object or the current elevation.
type: On/Off (0/1)
saved: Program settings
default: 0 (Off)
related: OSMODE, ELEVATION
---

## Values
| Value | Meaning |
|---|---|
| 0 | A snapped point keeps the Z value of the object it snaps to. |
| 1 | A snapped point is placed on the current UCS plane at the current elevation ([`ELEVATION`](ELEVATION-System-Variable)). |

## Notes
- A value other than 0 or 1 is refused.
