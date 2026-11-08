---
summary: Copies objects that are nested inside block references or xrefs into the current drawing, without exploding the reference.
related: NCOPYALL, COPY, EXPLODE, BURST, XREF
ribbon: Draw tab › Modify panel (slide-out)
---

## Usage
1. Current setting: Insert or Bind. Select nested objects or [Settings]. Click objects inside block references or xrefs. Each click picks the nested object nearest the cursor.
2. Press Enter when done selecting.
3. Specify base point or [Displacement/Multiple] <Displacement>.
4. Specify second point or [Array] <use first point as displacement>. The copies are placed.

## Options
| Option | What it does |
|---|---|
| **Settings** (`S`) | Enter setting [Insert/Bind]. Chooses how layer and linetype names from an xref are named in the copies. |
| **Insert** (`I`) | Copied xref layers get plain names (for example `Detail`). |
| **Bind** (`B`) | Copied xref layers keep the xref name, in the form `xref$0$Detail`. |
| **Displacement** (`D`) | Uses the next point you give as the displacement from 0,0. |
| **Multiple** (`M`) | Keeps placing copies from the same base point. **Undo** (`U`) removes the last copy, **Exit** (`E`) ends. |
| **Array** (`A`) | Enter number of items to array, then specify the second point. **Fit** (`F`) spreads the items so the last one lands on the point you pick. |

## Notes
- Nested references are copied down to their individual objects.
- The copies keep the layer and style of the nested objects.
- The Insert/Bind choice is remembered for the next run.
