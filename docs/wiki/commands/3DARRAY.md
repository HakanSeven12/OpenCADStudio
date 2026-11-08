---
summary: Copies the selected objects into a 3D grid of rows, columns and levels.
related: ARRAY, ARRAYRECT, ARRAYPOLAR, COPY
---

## Usage
1. Select the objects, or start the command and select them, then press Enter.
2. Enter the row count. Press Enter for 2.
3. Enter the column count. Press Enter for 2.
4. Enter the level count. Press Enter for 2.
5. Enter the row spacing. Press Enter for 1.
6. Enter the column spacing. Press Enter for 1.
7. Enter the level spacing. Press Enter for 1. The copies are created.

## Notes
- Rows run along the Y axis, columns along the X axis, and levels along the Z axis.
- The copies are separate objects, not an associative array.
- Counts below 1 are treated as 1. A text that is not a number falls back to the default value.
- If the total number of copies is too large, the command reports that the array size exceeds the limit.
- Only the rectangular form is available; there is no polar option.
