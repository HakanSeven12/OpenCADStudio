---
summary: Draws freehand lines, polylines or splines that follow the cursor while the pen is down.
related: PLINE, SPLINE, LINE
---

## Usage
1. Click to lower the pen.
2. Move the cursor to sketch. Points are recorded at the record increment.
3. Click again to lift the pen. Click once more to start a new stroke.
4. Press Enter (or type `X`) to save the sketch and end the command.

## Options
| Option | What it does |
|---|---|
| **Pen** (`P`) | Lowers or lifts the pen at the cursor. |
| **Type** (`T`) | Chooses what each stroke becomes: **Line** (`L`) separate line segments, **Polyline** (`P`) one polyline, or **Spline** (`S`) a fitted spline. |
| **Increment** (`I`) | Sets the minimum distance between recorded points. |
| **Tolerance** (`TO`) | Spline type only. Sets how closely the spline fits the points, from 0 to 1. |
| **Record** (`R`) | Saves the strokes drawn so far and keeps sketching. |
| **Erase** (`E`) | Lifts the pen. Move back over the end of the temporary sketch to remove points. Choose it again, or click, to stop erasing. |
| **Connect** (`C`) | Starts a new stroke at the end of the last stroke. |
| **Exit** (`X`) | Saves the sketch and ends the command. |
| **Quit** (`Q`) | Discards the unsaved strokes and ends the command. |

## Notes
- Escape discards strokes that are not yet recorded.
- The type, increment and tolerance are kept in `SKPOLY`, `SKETCHINC` and `SKTOLERANCE`.
- Sketches are drawn on the current UCS plane.
