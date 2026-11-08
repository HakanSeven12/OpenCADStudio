---
summary: Edits a spline: fit points, control vertices, closing, joining, direction, and conversion to a polyline.
related: SPLINE, SPLINECV, SPLINEFIT, PEDIT, JOIN
ribbon: Draw tab › Modify panel (slide-out)
---

## Usage
1. Select a spline. If you selected one spline before you start the command, this step is skipped.
2. Type an option. Each edit is applied at once, and the prompt comes back so you can make more edits.
3. Press Enter or type `X` to finish.

## Options
| Option | What it does |
|---|---|
| **Fit data** (`F`) | Only for splines with fit points. Opens the fit data options below. |
| **Close** (`C`) / **Open** (`O`) | Closes an open spline, or opens a closed one. |
| **Join** (`J`) | Open splines only. Select open curves to join to the spline and press Enter. |
| **Move vertex** (`M`) | Moves control vertices one at a time. Specify the new location, or type **Next** (`N`), **Previous** (`P`), **Select point** (`S`) or **eXit** (`X`). |
| **Refine** (`R`) | Opens the refine options below. |
| **rEverse** (`E`) | Reverses the direction of the spline. |
| **convert to Polyline** (`P`) | Enter a precision from 0 to 99. The spline is replaced by a polyline of straight segments. |
| **Undo** (`U`) | Undoes the last edit made in this command. |
| **eXit** (`X`) | Ends the command. |

Fit data options:

| Option | What it does |
|---|---|
| **Add** (`A`) | Pick an existing fit point, then specify new points. Type **After** (`A`) or **Before** (`B`) to choose the side. |
| **Delete** (`D`) | Pick fit points to delete. |
| **Move** (`M`) | Moves fit points one at a time, with **Next**, **Previous**, **Select point** and **eXit**. |
| **Purge** (`P`) | Removes the fit data. The spline keeps its shape as control vertices. |
| **Tangents** (`T`) | Open splines only. Specify the start and end tangents, or type `S` for the system default. |
| **eXit** (`X`) | Goes back to the main options. |

Refine options:

| Option | What it does |
|---|---|
| **Add** (`A`) | Click points on the spline to add control vertices. |
| **Delete** (`D`) | Pick control vertices to delete. |
| **Elevate order** (`E`) | Enter a higher order, up to 26. |
| **Move** (`M`) | Moves control vertices, as in Move vertex. |
| **Weight** (`W`) | Enter a new weight for each control vertex, with **Next**, **Previous**, **Select point** and **eXit**. |
| **eXit** (`X`) | Goes back to the main options. |

## Notes
- Splines on locked layers cannot be edited.
- When converting to a polyline, the original spline is kept if [`DELOBJ`](DELOBJ-System-Variable) is 0.
- Refine and Move vertex edits remove the fit data, because the spline is then defined by its control vertices.
