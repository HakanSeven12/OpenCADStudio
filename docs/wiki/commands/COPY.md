---
summary: Copies the selected objects to one or more new places, measured from a base point.
related: MOVE, ARRAY, COPYCLIP, COPYBASE, COPYTOLAYER
ribbon: Draw tab › Modify panel
---

## Usage
1. Select objects and press Enter. If you selected objects first, this step is skipped.
2. Specify the base point, or type an option.
3. Specify the destination. A copy is placed there and the command stays active.
4. Specify more destinations to place more copies.
5. Press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **Displacement** (`D`) | At the base point prompt: type or pick a displacement, for example `10,5`. One copy is placed that far from the originals and the command ends. |
| **mOde** (`O`) | At the base point prompt: choose **Single** (`S`), which ends after one copy, or **Multiple** (`M`), the default, which keeps placing copies. |
| **Array** (`A`) | At the destination prompt: enter the number of items, then pick the second point. The copies are placed in a row, each one step apart, and the command ends. The count includes the originals. |
| **Fit** (`F`) | After Array: the point you pick is the last item, and the copies are spread evenly up to it. |
| **Undo** (`U`) | At the destination prompt: removes the last copy placed by this command. |

## Notes
- A preview of the copies follows the cursor.
- Picking the base point also puts the selected objects on the clipboard with that base point, so you can paste them later with [`PASTECLIP`](PASTECLIP-Command).
