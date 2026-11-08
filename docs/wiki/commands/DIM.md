---
summary: Creates many kinds of dimensions in one command; the kind follows the object or points you pick.
related: DIMLINEAR, DIMALIGNED, DIMANGULAR, DIMRADIUS, DIMDIAMETER, DIMBASELINE, DIMCONTINUE, DIMSPACE, DIMSTYLE
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Pick an object, specify the first extension line origin, or type an option.
   - A horizontal or vertical line or a polyline segment gives a linear dimension; a slanted line gives an aligned dimension.
   - A circle gives a diameter dimension; an arc gives a radius dimension.
   - A point starts a dimension between two points. If the second point is not straight across or above the first, the dimension is aligned.
2. Place the dimension. The prompts and options of the matching dimension type apply.
3. DIM asks again for the next dimension. Press Enter at the main prompt to finish.

## Options
At the main prompt:

| Option | What it does |
|---|---|
| **Angular** (`A`) | Starts an angular dimension, as in [`DIMANGULAR`](DIMANGULAR-Command). |
| **Baseline** (`B`) | Creates a baseline series. Pick the first extension line origin of the base dimension, then the next origins. **Offset** (`O`) sets the spacing between the dimension lines. |
| **Continue** (`C`) | Creates a chain of dimensions. Pick the extension line origin to continue from, then the next origins. |
| **Ordinate** (`O`) | Starts an ordinate dimension, as in [`DIMORDINATE`](DIMORDINATE-Command). |
| **aliGn** (`G`) | Aligns dimensions: select a base dimension, then the dimensions to line up with it. |
| **Distribute** (`D`) | Spaces selected dimensions evenly (**Equal**) or by a set distance (**Offset**). |
| **Layer** (`L`) | Sets the layer for the dimensions DIM creates. Type a layer name, pick an object to use its layer, type `.` for the current layer, or `?` to list layers. **eXit** (`X`) or Enter goes back. |
| **Undo** (`U`) | Removes the last dimension and continues. |

After picking a circle or an arc, these extra options are offered while you place the dimension:

| Option | What it does |
|---|---|
| **Radius** (`R`) | Switches a circle to a radius dimension. |
| **Diameter** (`D`) | Switches an arc to a diameter dimension. |
| **Jogged** (`J`) | Starts a jogged radius dimension, as in [`DIMJOGGED`](DIMJOGGED-Command). |
| **Angular** (`A`) | Switches to an angular dimension of the same object. |
| **Arc length** (`L`) | Switches an arc to an arc length dimension. |
| **text aNgle** (`N`) | Sets the angle of the dimension text. |

## Notes
- When a new dimension overlaps existing ones, DIM asks what to do: **Move away** (`M`), **Break up** (`B`), **Replace** (`R`) or **None** (`N`). In a continued series only **Break up** and **None** are offered. Your choice becomes the default for the session.
- Each dimension uses the current dimension style.
