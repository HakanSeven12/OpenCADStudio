---
summary: Copies the selected objects along a path curve, aligned to the path, as an associative array or as separate copies.
related: ARRAYRECT, ARRAYPOLAR, ARRAYEDIT, MEASURE, DIVIDE
ribbon: Draw tab › Modify panel (Array drop-down)
---

## Usage
1. Select objects and press Enter, or select them before you start the command.
2. Select path entity. Click near the end of the path where the array should start.
3. With the Measure method:
   1. Enter the distance between items along the path.
   2. Enter the number of items. The prompt shows the most that fit; press Enter to use that number.
4. With the Divide method, enter the item count. The items are spread over the whole path.

## Options
| Option | What it does |
|---|---|
| **Method** (`M`) | Switches between Measure (items a fixed distance apart) and Divide (a number of items spread over the path). The button shows the current method. |
| **Associative** (`AS`) | Switches between an associative array (Yes) and separate copies (No). The button shows the current setting. |

## Notes
- The path can be a line, arc, circle, polyline, ellipse or spline. It cannot be one of the objects being copied.
- The path is highlighted while you hover over it.
- The suggested spacing is the one you used last; the first time it is 1.5 times the width of the selected objects.
- The count, spacing, method and the Associative setting are remembered for the next time.
- An associative array is one object. Select it to change it with its grips or in the Properties palette, and use [`ARRAYEDIT`](ARRAYEDIT-Command) to edit its source objects or single items.
- One array can hold at most 100,000 items.
