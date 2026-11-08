---
summary: Controls whether dynamic dimensional constraints are shown.
type: On/Off (0/1)
saved: Not saved
default: 1 (On)
related: DCDISPLAY, CCONSTRAINTFORM, CONSTRAINTBARDISPLAY
---

## Values
| Value | Meaning |
|---|---|
| 0 | All dynamic dimensional constraints are hidden. |
| 1 | Dynamic dimensional constraints are shown, except the ones you hid one by one. |

## Notes
- Annotational dimensional constraints are not affected.
- [`DCDISPLAY`](DCDISPLAY-Command) shows or hides single constraints.
- The value applies to the current drawing while it is open. A drawing opened later starts with 1.
