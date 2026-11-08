---
summary: Shows the name of the block or reference being edited in place.
type: String read-only
saved: Not saved
default: "" (empty)
related: REFEDIT, REFCLOSE, BLOCKSTATE
---

## Notes
- It is empty when no in-place edit is running.
- [`REFEDIT`](REFEDIT-Command) starts an in-place edit and [`REFCLOSE`](REFCLOSE-Command) ends it.
