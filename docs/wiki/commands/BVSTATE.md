---
summary: In the Block Editor, creates, sets current, renames or deletes the visibility states of a dynamic block.
related: BVHIDE, BVSHOW, BPARAMETER, BEDIT
ribbon: Block Editor tab › Visibility panel
---

## Usage
1. Open a block in the Block Editor and run `BVSTATE`. The current visibility state is shown.
2. At "Enter an option [New/Set/Rename/Delete/Dialog] <Set>:", type an option, or press Enter for **Set**.
3. Answer the prompts of that option (see below).

## Options
| Option | What it does |
|---|---|
| **New** (`N`) | Asks for the name of a new state, then how its objects start out: **Show all objects** (`SHOW`), **Hide all objects** (`HIDE`) or **Keep current visibility** (`KEEP`, the default). The new state becomes current. |
| **Set** (`S`) | Asks for the state to make current. Press Enter to keep the current one. |
| **Rename** (`R`) | Asks for the state to rename (Enter = current state), then the new name. |
| **Delete** (`D`) | Asks for the state to delete. The only remaining state cannot be deleted. |
| **Dialog** (`DIALOG`) | Opens the Visibility States dialog. |

## Notes
- State names are matched without regard to case. A new or renamed state cannot use a name that already exists.
- The Visibility States dialog lists the states and lets you make one current, rename, delete, move states up or down, and add a new state with a chosen start visibility. Click **OK** to apply. The **Visibility States** ribbon button opens this dialog directly.
- If the block has no visibility parameter yet, the first change adds one at the block's base point, which makes the block dynamic. The dialog needs an existing visibility parameter.
- Each change can be undone.
