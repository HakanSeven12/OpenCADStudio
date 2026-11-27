---
summary: Creates, copies, renames, deletes, lists or switches layouts from the command line, and inserts layouts from another drawing.
related: LAYOUTMANAGER, MVIEW, PAGESETUP
---

`LAYOUT` and `-LAYOUT` both ask at the command line. To manage layouts in a dialog, use [`LAYOUTMANAGER`](LAYOUTMANAGER-Command).

## Command-line form
`-LAYOUT` asks everything at the command line and never opens a dialog, so it suits scripts.

1. At `Enter layout option [Copy/Delete/New/Template/Rename/Set/?] <Set>:`, type an option.
2. Answer the prompts for that option.

| Option | What it asks |
|---|---|
| **Copy** | The layout to copy (default: the current one), then the name for the copy (default: the name with ` (2)` added). |
| **Delete** | The layout to delete (default: the current one). |
| **New** | The name of the new layout (default: the next free layout name). |
| **Template** | A drawing (`.dwt`, `.dwg` or `.dxf`); press Enter at the file prompt to pick it in a file dialog. Then the layouts to insert (default `*`, all of them; wildcards and comma-separated names work). Each layout comes with its page setup and its paper-space objects, and the layers, blocks and styles those objects use. A name the drawing already has gets ` (2)` added. **Insert › Layout › Layout from Template...** opens the file dialog directly. |
| **Rename** | The layout to rename (default: the current one), then the new name. |
| **Set** | The layout to make current (default: the current one). This is the default option. |
| **?** | Lists all layouts. |

The answers can also follow the command on one line.

## Notes
- Layout names are matched without regard to case.
- The Model layout cannot be deleted or renamed.
- `LAYOUT` and `-LAYOUT` are not allowed inside the Block Editor.
