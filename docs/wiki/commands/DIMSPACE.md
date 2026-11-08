---
summary: Spaces linear and aligned dimensions evenly from a base dimension, or lines them up.
related: DIMBASELINE, DIMCONTINUE, DIMSTYLE
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select the base dimension. It must be a linear or aligned dimension, and it stays where it is.
2. Select the dimensions to space, then press Enter. Only linear and aligned dimensions are kept.
3. Type the distance between dimensions, or press Enter for **Auto**.

The dimension lines are moved so they sit one spacing apart from the base dimension, in the order you selected them. The command line reports "Spacing adjusted."

## Options
| Option | What it does |
|---|---|
| **Auto** (`A`) | Uses the baseline spacing of the base dimension's style (`DIMDLI`), scaled by its overall scale. This is the default. |

## Notes
- Type `0` to line the dimension lines up with the base dimension.
- The spacing cannot be negative.
- Dimensions on locked layers are not selected.
