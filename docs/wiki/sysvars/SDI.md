---
summary: Controls whether one drawing or several drawings can be open at a time.
type: On/Off (0/1)
saved: Not saved
default: 0 (Off)
related: OPEN
---

## Values
| Value | Meaning |
|---|---|
| 0 | Several drawings can be open, each in its own tab. |
| 1 | One drawing at a time: [`OPEN`](OPEN-Command) closes the current drawing before it opens the new one. |

## Notes
- With 1, if the current drawing has unsaved changes, opening another drawing is refused until you save or close it.
- The value resets to 0 when the program starts again.
