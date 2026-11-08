---
summary: Restores the objects removed by the most recent erase, without undoing anything done since.
related: ERASE, UNDO, U
---

## Usage
1. Run the command. The objects of the last erase come back, and the command line reports how many were restored.

## Notes
- Works after [`ERASE`](ERASE-Command) and after deleting a selection with the Delete key.
- Only the most recent erase is remembered, and only once: running OOPS again says there is nothing to restore.
- OOPS itself can be undone.
