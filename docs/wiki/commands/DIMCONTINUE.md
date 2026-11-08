---
summary: Creates a chain of dimensions, each starting at the second extension line of the one before.
related: DIMBASELINE, DIMLINEAR, DIMANGULAR, DIMORDINATE, DIMSPACE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. The dimension you created last is continued. If there is none, select the dimension to continue.
2. Specify the second extension line origin of the next dimension. For an ordinate dimension, specify the feature location.
3. Repeat step 2 for more dimensions. Each new dimension lines up with the one before.
4. Press Enter to select another dimension to continue, or press Enter again to finish.

## Options
| Option | What it does |
|---|---|
| **Select** (`S`) | Lets you select a different dimension to continue from. |
| **Undo** (`U`) | Removes the last dimension of the chain. |

## Notes
- You can continue linear, aligned, angular and ordinate dimensions.
- [`DIMCONTINUEMODE`](DIMCONTINUEMODE-System-Variable) decides whether the new dimensions use the current dimension style (0) or the style of the dimension you continue from (1).
- [`DIM`](DIM-Command) offers the same chain through its **Continue** option.
