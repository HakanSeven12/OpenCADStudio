---
summary: Lines up selected multileaders along a line, spreads them evenly, or makes their leader lines parallel.
related: MLEADER, MLEADERCOLLECT, MLEADEREDIT
ribbon: Annotate tab › Leaders panel
---

## Usage
1. Select multileaders, then press Enter. The command line shows the current mode.
2. Select the multileader to align to, or type `O` to change the mode.
3. Specify the direction of the line the multileaders line up on. The other multileaders move onto it.

In Distribute mode, step 2 asks for a first point instead; specify it and then a second point.

## Options
| Option | What it does |
|---|---|
| **Options** (`O`) | Opens the mode choice below. |
| **Distribute** (`D`) | Spaces the multileaders evenly between two points you pick. |
| **make leader segments Parallel** (`P`) | Makes each leader's last segment parallel to that of the multileader you align to. Its length stays the same. |
| **specify Spacing** (`S`) | Sets a gap between the contents, then lines them up from the reference in the direction you pick. |
| **Use current spacing** (`U`) | Lines the contents up on the direction line and keeps their current spacing along it. |

## Notes
- Only the content moves; the arrowheads stay where they are.
- The mode and spacing carry over to the next run.
- Multileaders on locked layers are not moved.
