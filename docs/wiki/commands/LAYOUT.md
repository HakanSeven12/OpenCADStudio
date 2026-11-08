---
summary: Creates, copies, renames, deletes, lists or switches layouts from the command line.
related: LAYOUTMANAGER, MVIEW, PAGESETUP
---

Only the command-line form `-LAYOUT` is available. To manage layouts in a dialog, use [`LAYOUTMANAGER`](LAYOUTMANAGER-Command).

## Command-line form
`-LAYOUT` asks everything at the command line and never opens a dialog, so it suits scripts.

1. At `Enter layout option [Copy/Delete/New/Rename/Set/?] <Set>:`, type an option.
2. Answer the prompts for that option.

| Option | What it asks |
|---|---|
| **Copy** | The layout to copy (default: the current one), then the name for the copy (default: the name with ` (2)` added). |
| **Delete** | The layout to delete (default: the current one). |
| **New** | The name of the new layout (default: the next free layout name). |
| **Rename** | The layout to rename (default: the current one), then the new name. |
| **Set** | The layout to make current (default: the current one). This is the default option. |
| **?** | Lists all layouts. |

The answers can also follow the command on one line.

## Notes
- Layout names are matched without regard to case.
- The Model layout cannot be deleted or renamed.
- `-LAYOUT` is not allowed inside the Block Editor.
