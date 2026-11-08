---
summary: Creates a wall-like 3D solid of a set height and width, along a path you draw or along an existing line, arc, circle, ellipse, polyline or spline.
related: EXTRUDE, SWEEP, BOX, PLINE
ribbon: Model tab › Create panel (primitives drop-down)
---

## Usage
1. The command line shows the current height, width and justification.
2. `Specify start point or [Object/Height/Width/Justify] <Object>:` pick a start point, or type an option. Press Enter to choose an object.
3. `Specify next point or [Arc/Undo]:` pick the next point. After three points, **Close** is offered too.
4. Press Enter to finish the solid.

To follow an existing object, type `O` (or press Enter at the first prompt) and then `Select object:`.

## Options
| Option | What it does |
|---|---|
| **Object** (`O`) | Builds the solid along a line, arc, circle, ellipse, polyline or spline you select. |
| **Height** (`H`) | Sets the wall height. The default is 80. |
| **Width** (`W`) | Sets the wall width. The default is 5. |
| **Justify** (`J`) | Places the wall to the **Left**, **Center** or **Right** of the path. The default is Center. |
| **Arc** (`A`) | Continues with arc segments. |
| **Close** (`C`) | Closes the path back to the start point. |
| **Undo** (`U`) | Removes the last segment. |
| **Direction** (`D`) | In arc mode: sets the tangent direction at the arc's start. |
| **Line** (`L`) | In arc mode: goes back to straight segments. |
| **Second point** (`S`) | In arc mode: draws the arc through a second point and an end point. |

## Notes
- If a supported object is already selected when you start the command, pressing Enter at the first prompt uses it.
- With **Object**, the source object is deleted when [`DELOBJ`](DELOBJ-System-Variable) is not 0, unless it is on a locked layer.
- Height, width and justification are remembered for the next run.
