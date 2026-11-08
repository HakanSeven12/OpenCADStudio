# Measuring and Inquiry

Measuring tools report on the drawing without adding anything to it. Results appear by the cursor while you measure and are written to the command line.

## Measure

The **Measure** list on the Draw tab has the four everyday measurements; `MEASUREGEOM` (`MEA`) offers them all, plus Quick.

| | Tool | Command | Measures |
|---|---|---|---|
| <img src="images/icons/distance.svg" width="24" alt=""> | Distance | `DIST` (`DI`) | The distance between two points, with the angle and the X, Y and Z differences. |
| <img src="images/icons/measure-radius.svg" width="24" alt=""> | Radius | `MEASURERADIUS` | The radius and diameter of a circle or arc. |
| <img src="images/icons/measure-angle.svg" width="24" alt=""> | Angle | `MEASUREANGLE` | An angle from three points, or between two lines. |
| <img src="images/icons/area.svg" width="24" alt=""> | Area | `AREA` | The area and perimeter of points you pick or of closed objects. **Add** and **Subtract** keep a running total, so you can take the holes out of a floor area. |
| <img src="images/icons/measure.svg" width="24" alt=""> | Quick | `MEASUREGEOM` › `Q` | Live: as the cursor moves, the distances to the nearest geometry horizontally and vertically. Click to write the values to the command line. |

The last area and perimeter are kept in the `AREA` and `PERIMETER` system variables, so fields and scripts can use them.

## Inquiry

| Command | What it reports |
|---|---|
| `LIST` (`LI`) | Everything about the selected objects: type, layer, handle, coordinates, length, area and so on. |
| `ID` | The coordinates of a point. |
| `MASSPROP` | Area and perimeter of regions; volume, mass, centroid and moments of inertia of solids. |
| `DBLIST` | Every object in the drawing, as `LIST` does. |
| `DWGPROPS` | The drawing's properties and statistics. |

## QuickCalc

<img src="images/icons/quickcalc/quickcalc.svg" width="24" alt=""> `QUICKCALC` (`QC`) opens a calculator palette:

- arithmetic, scientific and trigonometric functions;
- <img src="images/icons/quickcalc/point.svg" width="20" alt=""> pick a point, <img src="images/icons/quickcalc/distance.svg" width="20" alt=""> a distance or <img src="images/icons/quickcalc/angle.svg" width="20" alt=""> an angle from the drawing into the expression;
- unit conversion;
- <img src="images/icons/quickcalc/var_new.svg" width="20" alt=""> variables you name and reuse;
- a history of results.

<img src="images/icons/quickcalc/send.svg" width="20" alt=""> **Send** passes the result to the command line, for example as the answer to a distance prompt.

`CAL` evaluates an expression at the command line, including point and vector expressions.

## Counting

`COUNT` and the **Count** palette (`COUNTLIST`) count the references of each block in the drawing or in an area; `COUNTTABLE` puts the count in a table.
