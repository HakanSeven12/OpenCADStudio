---
summary: Sets whether clicking an object adds it to the selection or replaces the selection.
type: Integer (0–2)
saved: Program settings
default: 2 (Add)
related: PICKDRAG, PICKBOX, PICKSTYLE
---

## Values
| Value | Meaning |
|---|---|
| 0 | A click replaces the selection. Shift+click adds an object, or removes it if it is already selected. |
| 1 | Picks add to the set and Shift removes objects. SELECT saves the set as Previous. |
| 2 | Same as 1 and leaves objects visibly selected when SELECT ends. |

## Notes
- Set it in [`OPTIONS`](OPTIONS-Command), Selection Modes: "Use Shift to add to selection". The box is checked when the value is 0.
