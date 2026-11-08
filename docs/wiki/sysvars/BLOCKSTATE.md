---
summary: Tells whether a block or external reference is being edited in place.
type: On/Off (0/1) read-only
saved: Not saved
default: 0 (No)
related: REFEDIT, BLOCKEDITOR
---

## Values
| Value | Meaning |
|---|---|
| 0 | No in-place edit is active |
| 1 | An in-place edit started with REFEDIT is active |

## Notes
- [`REFEDIT`](REFEDIT-Command) starts an in-place edit.
