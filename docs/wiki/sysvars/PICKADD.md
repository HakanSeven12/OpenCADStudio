---
summary: Sets whether clicking an object adds it to the selection or replaces the selection.
type: On/Off (0/1)
saved: Program settings
default: 1 (Add)
related: PICKDRAG, PICKBOX, PICKSTYLE
---

## Values
| Value | Meaning |
|---|---|
| 0 | A click replaces the selection. Shift+click adds an object, or removes it if it is already selected. |
| 1 | A click adds to the selection. Shift+click removes an object. |

## Notes
- Set it in [`OPTIONS`](OPTIONS-Command), Selection Modes: "Use Shift to add to selection". The box is checked when the value is 0.
- Only 0 and 1 are accepted.
