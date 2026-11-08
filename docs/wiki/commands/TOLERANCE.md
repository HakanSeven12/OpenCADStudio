---
summary: Opens the Geometric Tolerance dialog and places a feature control frame in the drawing.
related: DIMSTYLE, LEADER, QLEADER, MLEADER
ribbon: Annotate tab › Dimensions panel (slide-out)
---
## Usage
1. Fill in the Geometric Tolerance dialog.
2. Click **OK**.
3. At `Enter tolerance location:`, pick the point for the frame. A preview of the frame follows the cursor.

Press Enter or Esc at the location prompt to cancel without placing a frame.

## Dialog
| Field | What it does |
|---|---|
| **Geometric characteristic** | The symbol in the first cell: None, Straightness, Flatness, Circularity, Cylindricity, Profile of a surface, Profile of a line, Position, Concentricity, Symmetry, Parallelism, Perpendicularity, Angularity, Circular runout or Total runout. |
| **Tolerance 1 / Tolerance 2** | A **Diameter** check box, the tolerance value and a material condition (None, Maximum material condition, Least material condition, Regardless of feature size). |
| **Datum 1 / Datum 2 / Datum 3** | The datum letter and its material condition. |
| **Projected height** | A projected tolerance zone value, with a **Projected tolerance zone** check box that adds the symbol. |
| **Datum identifier** | A datum identifier shown below the frame. |

## Notes
- **OK** stays disabled until at least one field has a value.
- The frame uses the current layer and other current object properties, and is placed on the current working plane.
- Editing an existing frame opens the same dialog with an extra **Apply** button. Frames on locked layers cannot be edited.
