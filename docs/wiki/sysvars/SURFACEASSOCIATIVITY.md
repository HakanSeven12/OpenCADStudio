---
summary: Controls whether new surfaces stay linked to the objects they were made from.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: EXTRUDE, REVOLVE, SWEEP, LOFT, DELOBJ
---

## Values
| Value | Meaning |
|---|---|
| 0 | New surfaces are independent of their source objects. |
| 1 | New surfaces keep reading their profile (and axis or path), so they follow when you edit those objects. |

## Notes
- Applies to surfaces made with [`EXTRUDE`](EXTRUDE-Command), [`REVOLVE`](REVOLVE-Command), [`SWEEP`](SWEEP-Command) and [`LOFT`](LOFT-Command).
- With [`SWEEP`](SWEEP-Command), the path of a linked surface is kept even when [`DELOBJ`](DELOBJ-System-Variable) would delete it.
- Only 0 or 1 is accepted.
