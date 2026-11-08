---
summary: Locks the Block Editor so that blocks cannot be opened for editing.
type: On/Off (0/1)
saved: Program settings
default: 0 (Unlocked)
related: BEDIT, BLOCKEDITOR
---

## Values
| Value | Meaning |
|---|---|
| 0 | The Block Editor can be opened |
| 1 | The Block Editor is locked |

## Notes
- While locked, [`BEDIT`](BEDIT-Command) refuses to open and says the Block Editor is locked.
- While locked, double-clicking a block selects it and opens the Properties palette instead.
