---
summary: Copies the selected objects around a center point, as an associative array or as separate rotated copies.
related: ARRAYRECT, ARRAYPATH, ARRAYEDIT, ROTATE
ribbon: Draw tab › Modify panel (Array drop-down)
---

## Usage
1. Select objects and press Enter, or select them before you start the command.
2. Specify center point.
3. Enter item count. Press Enter to keep the value shown in angle brackets.
4. Enter the total angle in degrees.

A preview of the copies follows the cursor and each value you enter. The array is created after the angle.

## Options
| Option | What it does |
|---|---|
| **Associative** (`AS`) | Switches between an associative array (Yes) and separate copies (No). The button shows the current setting. |

## Notes
- The item count includes the original objects and must be at least 2.
- The items are rotated about the Z axis of the current UCS.
- The count, angle and the Associative setting are remembered for the next time.
- An associative array is one object. Select it to change the item count, angle and radius with its grips or in the Properties palette, and use [`ARRAYEDIT`](ARRAYEDIT-Command) to edit its source objects or single items.
- When the UCS is rotated, separate copies are made even if Associative is on.
- One array can hold at most 100,000 items.
