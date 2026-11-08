---
summary: Adds an angular dimensional constraint between two lines, to an arc, or from three points.
related: DCANGULAR, DCONSTRAINT, PARAMETERS
---

## Usage
1. Select the first line or polyline segment, select an arc, or press Enter to use 3Point.
2. With a line: select the second line.
   With 3Point: specify the angle vertex, then the first and the second angle constraint points.
   With an arc: the arc's center and ends are used.
3. Specify the dimension line location.
4. Enter a value, or a name and value such as `ang1=45`. Press Enter to keep the measured angle under the default name.

## Options
| Option | What it does |
|---|---|
| **3Point** (`3P`) | Defines the angle by a vertex and two points. This is the default. |

## Notes
- Any current selection is cleared when the command starts; you pick the objects inside the command.
- The constraint gets the next free angular parameter name unless you type one.
- The angle is shown with the drawing's angular precision.
