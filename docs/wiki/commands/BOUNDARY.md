---
summary: Creates a closed polyline or a region from the area around a point you pick inside it.
related: HATCH, REGION
ribbon: Draw tab › Draw panel (expanded)
---

## Usage
1. Click a point inside a closed area. The boundary found around the point is highlighted. If no closed boundary is found, the prompt says so.
2. Click inside more areas if you need more boundaries.
3. Press Enter to create the boundaries and end the command.

## Options
| Option | What it does |
|---|---|
| **Advanced options** (`A`) | Opens the advanced options. **Object type** (`O`) there asks whether to create a **Region** (`R`) or a **Polyline** (`P`). Press Enter to go back. |
| **Boundary** (`O`) | Lets you select the objects that are used to find boundaries. Press Enter to return to picking points. |
| **Island detection style** (`S`) | Switches between **Normal**, **Outer** and **Ignore**. **Normal** also traces every island inside the area, at all levels. **Outer** traces only the islands directly inside the outer boundary. **Ignore** traces only the outer boundary. Normal is the default. |
| **Gap tolerance** (`G`) | Sets how large a gap between objects may be and still count as closed. |

## Notes
- The default object type is **Polyline**.
- If objects are selected before you start the command, only those objects are used to find boundaries.
- Only objects on the current working plane and visible on screen are used to find boundaries.
- The original objects are not changed.
- The gap tolerance is the same value as [`HPGAPTOL`](HPGAPTOL-System-Variable).
