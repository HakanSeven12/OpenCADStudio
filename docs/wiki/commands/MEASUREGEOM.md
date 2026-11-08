---
summary: Measures distance, radius, angle, area or volume, or shows live measurements as you move the cursor over geometry.
related: DIST, AREA, MEASURERADIUS, MEASUREANGLE, MASSPROP
---

## Usage
1. Start the command. It opens in quick measure, or at `Enter an option [Distance/Radius/Angle/ARea/Volume/Quick/Mode/eXit] <Distance>:` when the Mode option is set to No.
2. Choose a measurement and follow its prompts. The result is printed in the command line.
3. After each result the option prompt comes back, with the last option as the default.
4. Choose eXit, or press Esc, to end.

## Options
| Option | What it does |
|---|---|
| **Distance** (`D`) | Pick two points. Shows the distance, the angle in and from the XY plane, and delta X, Y and Z. At the second point, **Multiple points** (`M`) measures a chain of points, with Arc, Length, Undo and Total. |
| **Radius** (`R`) | Select an arc, circle or circular region edge. Shows radius and diameter. |
| **Angle** (`A`) | Pick the vertex, then two ray points. **Lines** (`LINES`) measures the smaller angle between two selected lines instead. |
| **ARea** (`AR`) | Pick corner points of a boundary (line and arc segments), or use **Object** (`O`) to select objects. Shows area and perimeter (circumference for a circle). **Add area** (`A`) and **Subtract area** (`S`) keep a running total. |
| **Volume** (`V`) | Same boundary or object as ARea, then specify a height. A 3D solid gives its own volume. **Add volume** and **Subtract volume** keep a running total. |
| **Quick** (`Q`) | No picks: as you move the cursor, the nearest geometry along the X and Y axes is measured live, with lengths and radii of the objects it touches. Click to print the current reading. Press Enter to exit. |
| **Mode** (`M`) | Answers `Always default to quick measure behavior? [Yes/No]`. The answer is saved in your profile. |
| **eXit** (`X`) | Ends the command. |

## Notes
- While you pick points, a live readout shows the current distance or area.
- In multiple-point and boundary modes, a first arc or length segment continues in the direction the last drawn line or arc ended.
