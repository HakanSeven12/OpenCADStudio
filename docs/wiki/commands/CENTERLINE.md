---
summary: Creates an associative centerline between two lines or straight polyline segments.
related: CENTERMARK, CENTERRESET, CENTERDISASSOCIATE, CENTERREASSOCIATE
ribbon: Annotate tab › Centerlines panel
---

## Usage
1. Run `CENTERLINE`.
2. At "Select first line or [Layer]:", pick a line or a straight segment of a polyline.
3. At "Select second line:", pick another line or segment.
4. The centerline is drawn midway between the two, and the command ends.

## Options
| Option | What it does |
|---|---|
| **Layer** (`L`) | Sets the layer for new centerlines and center marks. Type a layer name, type `.` to use the current layer, or pick an object to use its layer. `?` lists the layers; **eXit** (`X`) or Enter goes back to the pick prompt. |

## Notes
- The two lines can be parallel or meet at an angle. Picking the same line or segment twice is refused.
- The centerline is associative: when you change the source lines, it follows them.
- Its extension past the source lines comes from [`CENTEREXE`](CENTEREXE-System-Variable). Linetype and scale come from [`CENTERLTYPE`](CENTERLTYPE-System-Variable) and [`CENTERLTSCALE`](CENTERLTSCALE-System-Variable). The Layer option sets [`CENTERLAYER`](CENTERLAYER-System-Variable).
- The centerline is placed in the current working plane.
