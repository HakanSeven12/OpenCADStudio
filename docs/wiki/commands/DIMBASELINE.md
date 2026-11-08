---
summary: Creates a series of dimensions that all start from the same first extension line as a base dimension.
related: DIMCONTINUE, DIMLINEAR, DIMANGULAR, DIMORDINATE, DIMSPACE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. The dimension you created last is used as the base. If there is none, select a base dimension.
2. Specify the second extension line origin of the next dimension. For an ordinate base, specify the feature location.
3. Repeat step 2 for more dimensions. Each new dimension line is placed further out than the one before.
4. Press Enter to select another base dimension, or press Enter again to finish.

## Options
| Option | What it does |
|---|---|
| **Select** (`S`) | Lets you select a different base dimension. |
| **Undo** (`U`) | Removes the last dimension of the series. |

## Notes
- The base can be a linear, aligned, angular or ordinate dimension.
- The spacing between the dimension lines comes from the dimension style's dimension line spacing.
- [`DIMCONTINUEMODE`](DIMCONTINUEMODE-System-Variable) decides whether the new dimensions use the current dimension style (0) or the style of the base dimension (1).
- [`DIM`](DIM-Command) offers the same series through its **Baseline** option, with an extra **Offset** option to change the spacing.
