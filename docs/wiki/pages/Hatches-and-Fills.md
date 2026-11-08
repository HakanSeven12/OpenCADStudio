# Hatches and Fills

A hatch fills a closed area with a pattern, a solid colour or a gradient. Section cuts, materials, ground, zones on a plan: all are hatches.

<img src="images/icons/hatch/hatch_cross.svg" width="24" alt=""> `HATCH` (`H`) and <img src="images/icons/hatch/hatch_gradient.svg" width="24" alt=""> `GRADIENT` (`GD`) open the Hatch panel.

## Choosing the area

| | Method | How |
|---|---|---|
| <img src="images/icons/blocks/pick_point.svg" width="20" alt=""> | **Pick internal points** | Click inside an area enclosed by objects. The boundary is found for you, holes included. |
| <img src="images/icons/blocks/select_objects.svg" width="20" alt=""> | **Select objects** | Pick closed objects (polylines, circles, regions) to use as the boundary. |
| <img src="images/icons/hatch/hatch_boundary.svg" width="20" alt=""> | **Draw boundary** | Click the corners of a boundary that does not exist yet. |

**Gap tolerance** lets Pick internal points close small gaps in the boundary (`HPGAPTOL`). If no area is found, zoom in on the gap or raise the tolerance.

**Islands** are closed areas inside the boundary:

| Style | Result |
|---|---|
| Normal | Fills from the outside in, alternating: the first island is left empty, an island inside it is filled again, and so on. |
| Outer | Fills only the outermost area; every island is left empty. |
| Ignore | Fills everything inside the outer boundary. |

## Fill type

| | Type | Settings |
|---|---|---|
| <img src="images/icons/hatch/hatch_lines.svg" width="20" alt=""> | **Pattern** | A named pattern from the pattern list, with angle and scale. |
| <img src="images/icons/hatch/hatch_cross.svg" width="20" alt=""> | **User defined** | Parallel lines at a spacing and angle you give; **Double** crosses them at 90°. |
| <img src="images/icons/hatch/hatch_solid.svg" width="20" alt=""> | **Solid** | A solid colour. |
| <img src="images/icons/hatch/hatch_gradient.svg" width="20" alt=""> | **Gradient** | A blend of one or two colours. |

The gradient shapes:

<img src="images/icons/gradient/gradient_linear.svg" width="24" alt=""> linear · <img src="images/icons/gradient/gradient_cylindrical.svg" width="24" alt=""> cylindrical · <img src="images/icons/gradient/gradient_curved.svg" width="24" alt=""> curved · <img src="images/icons/gradient/gradient_spherical.svg" width="24" alt=""> spherical · <img src="images/icons/gradient/gradient_hemispherical.svg" width="24" alt=""> hemispherical, each also inverted.

Every hatch can also have its own colour, background colour, layer and transparency.

## Options

| Option | Effect |
|---|---|
| **Associative** | The hatch follows its boundary: stretch or move the boundary and the hatch updates. |
| **Create separate hatches** | Each area you pick becomes its own hatch instead of one hatch with several areas. |
| **Keep boundaries** | Saves the boundary that was found as a polyline. |
| **Annotative** | The pattern keeps its printed size at every annotation scale; see [Annotation Scaling](Annotation-Scaling). |
| **Origin** | Where the pattern starts, so bricks or tiles line up with an edge. |
| **Draw order** | Puts the hatch behind or in front of its boundary or of everything else. |

## Editing hatches

Double-click a hatch, or use <img src="images/icons/modify_hatchedit.svg" width="24" alt=""> `HATCHEDIT` (`HE`), to change its pattern, scale, angle, origin and other settings. The grips of a hatch move its boundary points and its origin. `TRIM` works on hatches as well: trimming a hatch with a line cuts away part of its area.

`HATCHTOBACK` (`HB`) sends every hatch in the drawing behind the other objects.

## Your own patterns

You can add your own hatch patterns in the standard `.pat` format. Put the `.pat` files in either of these folders and restart the program:

- the `patterns` folder inside your settings folder, for example `%APPDATA%\OpenCADStudio\patterns` on Windows (see [Installation](Installation#where-your-settings-live) for the other systems; create the folder if it does not exist);
- the folder set under **Options › Files › Support files**.

Every pattern in those files then appears in the pattern list next to the built-in ones. A file can hold any number of patterns. Each pattern starts with a header line and continues with one line per line family:

```
*MYGRID, 10 x 10 grid with dashes
0,   0,0,   0,10,   6,-4
90,  0,0,   0,10,   6,-4
```

Each family line is `angle, x-origin, y-origin, x-offset, y-offset` followed by dash lengths: positive numbers are dashes, negative numbers are gaps, and no dash list means a solid line.

The same support folders take your own linetypes (`.lin` files, in a `linetypes` folder); see [Layers and Object Properties](Layers-and-Properties#linetypes).
