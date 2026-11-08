---
summary: Sets what the text of dynamic dimensional constraints shows.
type: Integer
saved: Not saved
default: 2 (Name and value)
related: DIMCONSTRAINT, DYNCONSTRAINTDISPLAY, DCFORM
---

## Values
| Value | Meaning |
|---|---|
| 0 | The parameter name only. |
| 1 | The value only. |
| 2 | Name and value, as `name=value`. |

## Notes
- The setting belongs to the open drawing tab and goes back to 2 when a drawing is opened again.
- Changing it updates the text of all dynamic dimensions at once.
