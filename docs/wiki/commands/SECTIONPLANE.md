---
summary: Creates a section object that acts as a cutting plane through 3D solids, surfaces and regions, without changing them.
related: SECTION, SECTIONCULL, SLICE
---

## Usage
1. The prompt shows the current section type.
2. Select a planar face, or pick any point to locate the section line. Or type an option.
3. If you picked a point, specify the through point. The section runs through both points.
4. If you picked a face, the section is placed on that face at once.

## Options
| Option | What it does |
|---|---|
| **Draw section** (`D`) | Draws a section line with several points. Specify the start point and the next points, press Enter, then pick a point on the side the section looks toward. Type `U` to undo a point. |
| **Orthographic** (`O`) | Aligns the section to **Front** (`F`), **Back** (`A`), **Top** (`T`), **Bottom** (`B`), **Left** (`L`) or **Right** (`R`). Press Enter for Top. |
| **Type** (`T`) | Chooses the section type: **Plane** (`P`), **Slice** (`S`), **Boundary** (`B`) or **Volume** (`V`). Press Enter for Plane. |

## Notes
- Each new section object gets a name such as "Section Plane (1)".
- The source objects are not modified. Use [`SECTIONCULL`](SECTIONCULL-Command) to remove what lies outside a section.
