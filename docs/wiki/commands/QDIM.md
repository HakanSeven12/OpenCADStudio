---
summary: Creates a series of dimensions for the selected objects in one step: continuous, staggered, baseline, ordinate, radius or diameter.
related: DIMLINEAR, DIMCONTINUE, DIMBASELINE, DIMORDINATE, DIMRADIUS, DIMDIAMETER
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select the geometry to dimension and press Enter. If objects are already selected, this step is skipped.
2. Specify the dimension line position, or type an option. A preview follows the cursor.
3. Click to place the dimensions. The command ends.

The dimensions run horizontally or vertically. The direction depends on where you place the line: above or below the points gives horizontal dimensions, left or right of them gives vertical ones.

## Options
| Option | What it does |
|---|---|
| **Continuous** (`C`) | Dimensions each gap between neighbouring points, in one row. This is the default. |
| **Staggered** (`S`) | Like Continuous, but each next dimension is moved one dimension line spacing further out. |
| **Baseline** (`B`) | Measures every point from one datum point. |
| **Ordinate** (`O`) | Creates an ordinate dimension at each point. |
| **Radius** (`R`) | Creates a radius dimension for each selected circle and arc. |
| **Diameter** (`D`) | Creates a diameter dimension for each selected circle and arc. |
| **datumPoint** (`P`) | Asks for a new datum point for Baseline. |
| **Edit** (`E`) | Shows a cross at each dimension point. Click a point to remove it. Type **eXit** (`X`) or press Enter to go back. |
| **seTtings** (`T`) | Sets which points are used: **Endpoint** (`E`) or **Intersection** (`I`). Enter keeps Endpoint. |

## Notes
- With Endpoint, the end points of the selected objects are used. With Intersection, the points where the selected objects cross each other are used; if there are none, the end points are used.
- The seTtings choice is remembered for the next run.
- Selected dimensions also give their measured points.
- Points that line up in the dimension direction are dimensioned only once.
- The dimension line spacing for Staggered comes from the current dimension style.
- Without a datum point, Baseline measures from the first point.
- The dimensions use the current dimension style and stay associated with the objects they measure.
