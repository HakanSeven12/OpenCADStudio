---
summary: Moves objects into or out of the working set while a reference is being edited in place.
related: REFEDIT, REFCLOSE
---

## Usage
1. Start a session with [`REFEDIT`](REFEDIT-Command).
2. Run the command. At `Enter an option [Add/Remove] <Add>:`, type an option or press Enter for Add.
3. Select objects and press Enter.

## Options
| Option | What it does |
|---|---|
| **Add** (`A`) | Moves the selected drawing objects into the working set, so they are saved into the reference. |
| **Remove** (`R`) | Moves the selected objects out of the working set, so they stay in the drawing. |

## Notes
- The **Add to Working Set** and **Remove from Working Set** buttons on the toolbar at the right edge go straight to object selection.
- Without an active session, the command reports that no reference is checked out.
