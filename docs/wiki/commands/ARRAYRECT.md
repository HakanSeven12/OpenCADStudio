---
summary: Copies the selected objects in rows and columns, as an associative array or as separate copies.
related: ARRAYPOLAR, ARRAYPATH, ARRAYEDIT, 3DARRAY, COPY
ribbon: Draw tab › Modify panel (Array drop-down)
---

## Usage
1. Select objects and press Enter, or select them before you start the command.
2. Enter row count. Press Enter to keep the value shown in angle brackets.
3. Enter column count.
4. Enter the row spacing.
5. Enter the column spacing.

A preview of the copies follows each value you enter. The array is created after the column spacing.

## Options
| Option | What it does |
|---|---|
| **Associative** (`AS`) | Switches between an associative array (Yes) and separate copies (No). The button shows the current setting. |

## Notes
- `ARRAY` runs the same command.
- Rows run along the Y axis and columns along the X axis of the current UCS. Negative spacing places copies in the other direction.
- The counts, spacings and the Associative setting are remembered for the next time.
- An associative array is one object. Select it to change counts and spacing with its grips or in the Properties palette, and use [`ARRAYEDIT`](ARRAYEDIT-Command) to edit its source objects or single items.
- When the UCS is rotated, separate copies are made even if Associative is on.
- One array can hold at most 100,000 items. Larger counts are refused with an error.
