---
summary: Undoes the last action, or the given number of actions.
related: REDO, OOPS, U
---
## Usage
- Type `UNDO` to take back the last action.
- Type `UNDO <number>` to take back that many actions at once, for example `UNDO 5`.

## Notes
- If you ask for more steps than there are, everything that can be undone is undone. `UNDO 0` does nothing.
- When there is nothing left, the command line shows `Nothing to undo.`
- While a drawing command is running, undo steps that command back first (for example, it removes the last vertex of a polyline you are drawing) instead of undoing earlier work.
- While you edit a table cell, undo works on the text you are typing.
- Each drawing tab has its own undo history.
- Use [`REDO`](REDO-Command) to bring back what you undid.
