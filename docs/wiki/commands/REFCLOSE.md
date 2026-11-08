---
summary: Ends in-place reference editing, saving or discarding the changes.
related: REFEDIT, REFSET
---

## Usage
1. Run the command while a reference is being edited with [`REFEDIT`](REFEDIT-Command).
2. At `Enter option [Save/Discard reference changes] <Save>:`, type an option or press Enter to save.

## Options
| Option | What it does |
|---|---|
| **Save** (`S`) | Writes the working set back into the definition and updates every reference to it. For an external reference, the referenced file is written and reloaded. |
| **Discard** (`D`) | Drops the changes. The definition stays as it was. |

## Notes
- The **Save Block Edit** and **Discard Block Edit** buttons on the toolbar at the right edge do the same without the prompt.
- The command line reports how many references were updated.
- Undo of REFCLOSE opens the editing session again.
- Without an active session, the command reports that no reference is checked out.
