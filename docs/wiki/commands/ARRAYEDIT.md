---
summary: Edits an associative array: its source objects, single items, or all items back to their places.
related: ARRAYRECT, ARRAYPOLAR, ARRAYPATH, BEDIT, EXPLODE
---

## Usage
1. Select array, or select one array before you start the command.
2. Enter an option, or press Enter to exit.

## Options
| Option | What it does |
|---|---|
| **Source** (`S`) | Opens the objects that every item repeats in the Block Editor. When you save and close it, all items update. |
| **Move item** (`M`) | Select item, then specify a base point and a second point. Only that item moves. |
| **Erase item** (`E`) | Select item. Only that item is removed from the array. |
| **Reset** (`R`) | Puts back every moved or erased item. |
| **eXit** (`X`) | Ends the command. |

## Notes
- Counts and spacing are not edited here. Change them in the Properties palette or with the array's grips.
- Moved and erased items keep their changes when you later change counts or spacing, until you use Reset.
- If the selected object is not an associative array, an error is shown.
- Item edits can be undone.
