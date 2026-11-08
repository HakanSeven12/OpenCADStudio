# Modifying Objects

The editing tools are on the **Draw** tab, in the **Modify** panel. Each one works on a selection: select first and then run the tool, or run the tool and select when it asks.

## Move, copy, rotate, scale

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/move.svg" width="24" alt=""> | Move | `MOVE` (`M`) | Moves objects from a base point to a second point, or by a displacement you type. |
| <img src="images/icons/copy.svg" width="24" alt=""> | Copy | `COPY` (`CO`, `CP`) | Copies objects from a base point to one or more new places. **Array** places a row of evenly spaced copies. |
| <img src="images/icons/rotate.svg" width="24" alt=""> | Rotate | `ROTATE` (`RO`) | Turns objects about a base point. **Copy** keeps the original; **Reference** turns from a known angle to a new one. |
| <img src="images/icons/scale.svg" width="24" alt=""> | Scale | `SCALE` (`SC`) | Resizes objects about a base point. **Copy** keeps the original; **Reference** scales a known length to a new length. |
| <img src="images/icons/mirror.svg" width="24" alt=""> | Mirror | `MIRROR` (`MI`) | Mirrors objects about a line of two points, and asks whether to keep the source. Text stays readable unless `MIRRTEXT` is 1. |
| <img src="images/icons/stretch.svg" width="24" alt=""> | Stretch | `STRETCH` (`S`) | Moves the points inside a crossing window and stretches what connects to them. Objects entirely inside the window move. |
| <img src="images/icons/modify_align.svg" width="24" alt=""> | Align | `ALIGN` (`AL`) | Moves, turns and optionally scales objects by matching source points to destination points. |

## Trim, extend, break, join

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/trim.svg" width="24" alt=""> | Trim | `TRIM` (`TR`) | Cuts objects back to where they meet other objects. |
| <img src="images/icons/extend.svg" width="24" alt=""> | Extend | `EXTEND` (`EX`) | Lengthens objects to meet other objects. |
| <img src="images/icons/modify_break.svg" width="24" alt=""> | Break | `BREAK` (`BR`) | Removes the part of an object between two points. |
| <img src="images/icons/modify_breakatpoint.svg" width="24" alt=""> | Break at Point | `BREAKATPOINT` | Splits an object in two at one point. |
| <img src="images/icons/modify_join.svg" width="24" alt=""> | Join | `JOIN` (`J`) | Joins lines, arcs, polylines and splines that meet end to end into one object. |
| <img src="images/icons/modify_lengthen.svg" width="24" alt=""> | Lengthen | `LENGTHEN` (`LEN`) | Changes the length of a line or arc by a **Delta**, a **Percent**, a **Total** length, or **Dynamic**ally. |

**Trim and extend modes.** In **Quick** mode every object counts as a cutting edge: just click the parts to remove, or drag across them. In **Standard** mode you first pick the cutting (or boundary) edges, then the parts to trim (or extend). Hold Shift in Trim to extend instead, and in Extend to trim instead. `EXTRIM` trims everything on one side of a cutting object at once.

## Offset

<img src="images/icons/offset.svg" width="24" alt=""> `OFFSET` (`O`) makes a parallel copy at a distance: parallel lines, concentric circles and arcs, larger or smaller polylines.

Give a distance, pick the object, then click the side. **Through** places the copy through a point instead. **Erase** removes the source after offsetting; **Layer** puts the copy on the current layer or on the source's layer; **Multiple** keeps making copies, each one the distance from the last.

## Fillet and chamfer

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/fillet.svg" width="24" alt=""> | Fillet | `FILLET` (`F`) | Rounds the corner between two objects with an arc of a radius you set. A radius of 0 makes a sharp corner. |
| <img src="images/icons/chamfer.svg" width="24" alt=""> | Chamfer | `CHAMFER` (`CHA`) | Bevels the corner with a straight edge, given by two distances or a distance and an angle. |
| | Blend | `BLEND` (`BLE`) | Joins the ends of two curves with a smooth spline. |

Both work on lines, arcs, circles, polylines, construction lines and rays. **Polyline** treats every corner of a polyline at once. For solid edges, see [3D Modelling](3D-Modelling#edges).

## Arrays

<img src="images/icons/array_rect.svg" width="24" alt=""> `ARRAY` (`AR`) makes many copies in a pattern:

| | Array | Command |
|---|---|---|
| <img src="images/icons/array_rect.svg" width="20" alt=""> | Rectangular: rows, columns and levels | `ARRAYRECT` |
| <img src="images/icons/array_polar.svg" width="20" alt=""> | Polar: around a centre | `ARRAYPOLAR` |
| <img src="images/icons/array_path.svg" width="20" alt=""> | Path: along a curve | `ARRAYPATH` |

Arrays are **associative**: they stay one object that you can edit later. Select the array to change the count and spacing in the ribbon or in Properties, or drag its grips. `ARRAYEDIT` replaces or edits single items. Explode an array to turn it into separate objects.

## Erase, explode, undo

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/erase.svg" width="24" alt=""> | Erase | `ERASE` (`E`), **Delete** | Deletes objects. `OOPS` brings back the last erased set. |
| <img src="images/icons/explode.svg" width="24" alt=""> | Explode | `EXPLODE` (`X`) | Breaks blocks, polylines, dimensions, hatches and arrays into their parts. |
| <img src="images/icons/ui/undo.svg" width="24" alt=""> | Undo | `UNDO` (`U`), **Ctrl+Z** | Undoes the last action. **Ctrl+Y** or **Ctrl+Shift+Z** redoes it. |

## Polylines and splines

<img src="images/icons/modify_pedit.svg" width="24" alt=""> `PEDIT` (`PE`) edits polylines: close or open them, join segments to them, set the width, edit vertices, fit a curve or a spline through them, or straighten them again. It can turn lines and arcs into a polyline.

<img src="images/icons/modify_splinedit.svg" width="24" alt=""> `SPLINEDIT` (`SPE`) edits splines: fit points, control vertices, closing, smoothness and direction.

<img src="images/icons/modify_reverse.svg" width="24" alt=""> `REVERSE` flips the direction of lines, polylines, splines and helices, which matters for linetypes with text and for path arrays.

## Draw order

<img src="images/icons/modify_draworder.svg" width="24" alt=""> `DRAWORDER` (`DR`) sets which objects are drawn on top: bring to front, send to back, above or under another object. `TEXTTOFRONT` brings all text and dimensions to the top in one go; `HATCHTOBACK` (`HB`) sends all hatches to the back.

## Cleaning up

<img src="images/icons/overkill.svg" width="24" alt=""> `OVERKILL` removes duplicate and overlapping lines, arcs and polylines, and merges collinear pieces.

## Grips

Select an object without a command running and small squares appear at its key points: these are **grips**. Click a grip and move the cursor to change the object there: move an endpoint, stretch a corner, resize a circle, move the whole object by its centre grip. Several grips can be made hot together by holding **Shift** while you click them.

Hover a grip to see a menu of what that grip can do, for example add or remove a vertex, turn a straight segment into an arc, or lengthen. Dynamic blocks, arrays, tables and the UCS icon have grips of their own.

## Properties and matching

<img src="images/icons/properties.svg" width="24" alt=""> **Properties** (**Ctrl+1**) shows everything about the selected objects and lets you change it: layer, colour, coordinates, radius, text content.

<img src="images/icons/match_prop.svg" width="24" alt=""> `MATCHPROP` (`MA`) copies the properties of one object to others: pick the source, then the targets. **Settings** chooses which properties are copied.

<img src="images/icons/modify_setbylayer.svg" width="24" alt=""> `SETBYLAYER` sets the colour, linetype, lineweight and transparency of objects back to ByLayer.

## Clipboard

**Ctrl+C**, **Ctrl+X** and **Ctrl+V** copy, cut and paste between drawings. **Ctrl+Shift+C** copies with a base point you pick; **Ctrl+Shift+V** pastes as a block. `PASTEORIG` pastes at the same coordinates as the original.
