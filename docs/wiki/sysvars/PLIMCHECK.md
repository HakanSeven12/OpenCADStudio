---
summary: Turns limits checking on or off for the current paper space layout.
type: On/Off (0/1)
saved: Drawing
default: 0 (Off)
related: LIMCHECK, LIMITS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Points outside the layout limits are accepted. |
| 1 | Points outside the layout limits are refused with "Outside limits." |

## Notes
- Each layout keeps its own value.
- [`LIMITS`](LIMITS-Command) ON or OFF in a layout sets this value. In model space it sets [`LIMCHECK`](LIMCHECK-System-Variable) instead.
