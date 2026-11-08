---
summary: Cuts 3D solids, surfaces or meshes with a plane or a surface, and keeps one side or both.
related: SECTIONPLANE, SECTION, INTERFERE, SUBTRACT
---

## Usage
1. Select solids, surfaces or meshes to slice, then press Enter. If you selected them before you start the command, this step is skipped.
2. Specify the first point on the slicing plane, or type an option. Press Enter to use **3points**.
3. Finish defining the plane with the prompts of the option you chose.
4. Specify a point on the side you want to keep, or press Enter (or type `B`) to keep both sides.

## Options
| Option | What it does |
|---|---|
| **planar Object** (`O`) | Uses the plane of a planar curve you select: an arc, circle, ellipse, spline or polyline. |
| **Surface** (`S`) | Cuts with a surface you select. Then select a sliced piece to keep, or keep both. |
| **Zaxis** (`Z`) | Specify a point on the plane, then a point on its normal axis. |
| **View** (`V`) | Uses a plane parallel to the current view through a point you specify. |
| **XY** / **YZ** / **ZX** (`XY`, `YZ`, `ZX`) | Uses a plane parallel to that plane of the current UCS through a point you specify. |
| **3points** (`3`) | Specify three points on the plane. |
| **Both** (`B`) | Keeps both pieces after the cut. |

## Command-line form
`SLICE X|Y|Z <value> [TOP|BOTTOM]` cuts the selected objects at once with a world plane, for example `SLICE Z 10`. The plane is perpendicular to the given axis at that value (Z if you give no axis). The lower side is kept unless you add `TOP`. This form suits scripts.

## Notes
- If the plane does not cross every selected object, nothing is changed and a message says so.
- Objects on locked layers are not sliced.
- The cut can be undone.
