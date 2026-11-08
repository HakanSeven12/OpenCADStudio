# Dimensions

A dimension measures the drawing and shows the value. It is made of extension lines, a dimension line with arrows, and the text. Dimensions are **associative** by default: they are tied to the points of the objects they measure, and update when those objects move or change.

The dimension tools are on the **Annotate** tab, in the **Dimensions** panel.

## Dimension types

| | Type | Command | Measures |
|---|---|---|---|
| <img src="images/icons/dim_linear.svg" width="24" alt=""> | Linear | `DIMLINEAR` (`DLI`) | A horizontal or vertical distance. |
| <img src="images/icons/dim_aligned.svg" width="24" alt=""> | Aligned | `DIMALIGNED` (`DAL`) | The true distance, parallel to the two points. |
| <img src="images/icons/dim_angular.svg" width="24" alt=""> | Angular | `DIMANGULAR` (`DAN`) | The angle between two lines, of an arc, or between three points. |
| <img src="images/icons/dim_radius.svg" width="24" alt=""> | Radius | `DIMRADIUS` (`DRA`) | The radius of a circle or arc. |
| <img src="images/icons/dim_diameter.svg" width="24" alt=""> | Diameter | `DIMDIAMETER` (`DDI`) | The diameter of a circle or arc. |
| <img src="images/icons/dim_jog.svg" width="24" alt=""> | Jogged | `DIMJOGGED` (`DJO`) | The radius of a large arc whose centre is far away. |
| | Arc Length | `DIMARC` | The length along an arc. |
| <img src="images/icons/dim_ordinate.svg" width="24" alt=""> | Ordinate | `DIMORDINATE` (`DOR`) | The X or Y distance of a point from the origin of the UCS. |
| <img src="images/icons/dim_baseline.svg" width="24" alt=""> | Baseline | `DIMBASELINE` (`DBA`) | More dimensions from the same first point as the last one. |
| <img src="images/icons/dim_continue.svg" width="24" alt=""> | Continue | `DIMCONTINUE` (`DCO`) | A chain of dimensions, each starting where the last one ended. |
| <img src="images/icons/qdim.svg" width="24" alt=""> | Quick Dimension | `QDIM` | A series of dimensions for several objects at once. |

`DIM` does them all from one command: hover an object and it offers the dimension that fits (linear for a line, radius for an arc, angular between two lines), then click to place it.

## Placing a dimension

1. Start the command.
2. Pick two points (use object snaps), or press Enter and pick an object.
3. Move the cursor to place the dimension line, and click.

Before the last click you can type **Mtext** or **Text** to change the text, or **Angle** to turn it. In the text, `<>` stands for the measured value: `<> TYP` gives "25 TYP".

## Editing dimensions

- Drag the grips: the text grip moves the text, the line grip moves the dimension line, the extension grips move the measured points.
- Double-click the text to edit it in place.
- `DIMTEDIT` moves or turns the text; `DIMEDIT` (`DED`) changes the text, oblique angle or position of several dimensions.
- <img src="images/icons/dim_space.svg" width="24" alt=""> `DIMSPACE` makes the gaps between parallel dimensions equal.
- <img src="images/icons/dim_break.svg" width="24" alt=""> `DIMBREAK` breaks dimension and extension lines where they cross other objects.
- `DIMJOGLINE` adds a jog to the dimension line of a linear dimension.
- `DIMINSPECT` adds an inspection frame.
- `DIMREASSOCIATE` ties a dimension to objects again; `DIMDISASSOCIATE` frees it.
- `DIMOVERRIDE` changes one setting of a dimension without changing its style.

## Centre marks and centre lines

| | Tool | Command | What it draws |
|---|---|---|---|
| | Center Mark | `CENTERMARK` | An associative cross and lines through the centre of a circle or arc. |
| | Centerline | `CENTERLINE` | An associative centre line between two lines. |
| | Simple centre cross | `DIMCENTER` (`DCE`) | A plain cross of two lines at the centre; it does not follow later changes to the circle. |

Associative centre marks and centre lines follow their circle or lines when those change.

## Dimension styles

<img src="images/icons/dim_style.svg" width="24" alt=""> `DIMSTYLE` (`D`) opens the dimension style manager.

![The dimension style manager](images/screens/dimension-style.png)
 A style controls every part of a dimension, in groups:

| Group | Settings |
|---|---|
| Lines | Colours, linetypes and lineweights of dimension and extension lines; extension beyond the dimension line; offset from the measured point; fixed-length extension lines; suppressing either line. |
| Symbols and Arrows | First, second and leader arrowheads (or your own block), arrow size, centre marks, arc-length symbol, radius jog angle. |
| Text | Text style, height, colour, background, gap; vertical and horizontal placement; alignment. |
| Fit | What moves out when text and arrows do not fit between the extension lines; text placement when moved; overall scale. |
| Primary Units | Unit format, precision, decimal separator, rounding, prefix and suffix, measurement scale, zero suppression. |
| Alternate Units | A second value in other units, for example millimetres with inches in brackets. |
| Tolerances | Symmetrical, deviation, limits or basic tolerances, with precision and zero suppression. |

Changing a style updates every dimension that uses it. **Compare** lists the differences between two styles.

The overall scale (`DIMSCALE`) multiplies sizes such as arrow size and text height. For drawings printed at several scales, use annotative dimension styles instead; see [Annotation Scaling](Annotation-Scaling).

## Associativity

`DIMASSOC` controls how new dimensions are tied to geometry: 2 (the default) ties them to the objects, 1 makes them one object that is not tied, 0 explodes them into lines and text.

A dimension on a sheet layout can measure model objects through a viewport and still stay associative; when the model changes, the dimension on the sheet updates.
