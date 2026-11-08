---
summary: Adds an associative center mark to circles and arcs.
related: CENTERLINE, CENTERRESET, CENTERDISASSOCIATE, CENTERREASSOCIATE, DIMCENTER
ribbon: Annotate tab › Centerlines panel
---

## Usage
1. Run `CENTERMARK`.
2. At "Select circle or arc to add center mark or [Layer]:", pick a circle or arc. A center mark is added.
3. Pick more circles or arcs to add more marks.
4. Press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **Layer** (`L`) | Sets the layer for new center marks and centerlines. Type a layer name, type `.` to use the current layer, or pick an object to use its layer. `?` lists the layers; **eXit** (`X`) or Enter goes back to the pick prompt. |

## Notes
- The mark is associative: when you move or resize the circle or arc, the mark follows it.
- Its size and look come from [`CENTERCROSSSIZE`](CENTERCROSSSIZE-System-Variable), [`CENTERCROSSGAP`](CENTERCROSSGAP-System-Variable), [`CENTEREXE`](CENTEREXE-System-Variable), [`CENTERMARKEXE`](CENTERMARKEXE-System-Variable), [`CENTERLTYPE`](CENTERLTYPE-System-Variable) and [`CENTERLTSCALE`](CENTERLTSCALE-System-Variable). The Layer option sets [`CENTERLAYER`](CENTERLAYER-System-Variable).
- If you pick something that is not a circle or arc, you see "Object selected is not a circle or arc."
