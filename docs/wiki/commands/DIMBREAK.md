---
summary: Adds or removes breaks in dimension and extension lines where other objects cross them.
related: DIMSPACE, DIMJOGLINE, DIMSTYLE
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select a dimension, or type `M` to select several.
2. Select the objects that should break the dimension, then press Enter. Or press Enter right away for an automatic break at every crossing object.
3. The breaks are added and the command ends.

## Options
| Option | What it does |
|---|---|
| **Multiple** (`M`) | Select several dimensions and press Enter. They are broken together. |
| **Auto** (`A`) | Breaks the dimension at every object that crosses it. This is the default. |
| **Manual** (`M`) | Specify the first and second break points yourself. Only for a single dimension. |
| **Remove** (`R`) | Removes all breaks from the selected dimensions. |

## Notes
- The first object you pick must be a dimension.
- With **Multiple**, only dimensions are selected, and dimensions on locked layers are skipped.
