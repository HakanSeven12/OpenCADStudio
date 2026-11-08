---
summary: Creates an ordinate dimension that shows the X or Y distance of a point from the origin of the current UCS.
related: DIMBASELINE, DIMCONTINUE, UCS, DIMSTYLE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Specify the feature location: the point to dimension.
2. Specify the leader endpoint, or type an option.
3. Click to place the dimension. The command ends.

## Options
| Option | What it does |
|---|---|
| **Xdatum** (`X`) | Always shows the X value. |
| **Ydatum** (`Y`) | Always shows the Y value. |
| **MText** (`M`) | Opens the text editor so you can replace or add to the measured text. |
| **Text** (`T`) | Asks for the dimension text. Leave it blank, or type `<>`, to use the measured value. |
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. |

## Notes
- Without Xdatum or Ydatum, the leader direction decides: a mostly vertical leader shows the X value, a mostly horizontal one shows the Y value.
- Values are measured from the origin of the current UCS. Move the UCS origin to set the datum.
